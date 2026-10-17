#include <kotavel/http/server.hpp>

#include <chrono>
#include <ctime>
#include <iomanip>
#include <iostream>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

namespace kotavel::http {
namespace {
std::mutex access_log_mutex;

int normalized_status(int status) noexcept {
    return status >= 100 && status <= 599 ? status : 500;
}

std::string_view method_name(Method method) noexcept {
    switch (method) {
        case Method::get: return "GET";
        case Method::post: return "POST";
        case Method::put: return "PUT";
        case Method::patch: return "PATCH";
        case Method::delete_: return "DELETE";
    }
    return "UNKNOWN";
}

void print_access_log(const RequestView& request, int status,
                      std::chrono::system_clock::time_point received_at,
                      std::chrono::steady_clock::duration elapsed) noexcept {
    try {
        using namespace std::chrono;
        const auto timestamp = floor<milliseconds>(received_at);
        const auto whole_seconds = floor<seconds>(timestamp);
        const auto milliseconds_part = duration_cast<milliseconds>(timestamp - whole_seconds).count();
        const auto time_value = system_clock::to_time_t(received_at);
        std::tm utc_time{};
#if defined(_WIN32)
        const bool has_utc_time = gmtime_s(&utc_time, &time_value) == 0;
#else
        const bool has_utc_time = gmtime_r(&time_value, &utc_time) != nullptr;
#endif

        // Sanitizar controles evita que una ruta pueda falsificar líneas en la terminal.
        std::string printable_path(request.path);
        for (char& character : printable_path) {
            if (character == '\r' || character == '\n' || character == '\t') character = ' ';
        }

        const auto elapsed_ms = duration<double, std::milli>(elapsed).count();
        std::ostringstream line;
        line << "[HTTP] ";
        if (has_utc_time) {
            line << std::put_time(&utc_time, "%Y-%m-%dT%H:%M:%S") << '.'
                 << std::setw(3) << std::setfill('0') << milliseconds_part << 'Z';
        } else {
            line << "unknown-time";
        }
        line << ' ' << method_name(request.method) << ' ' << printable_path << ' '
             << normalized_status(status) << ' ' << std::fixed << std::setprecision(3)
             << elapsed_ms << " ms\n";

        // El servidor puede atender en paralelo; el lock conserva cada línea completa.
        const std::lock_guard lock(access_log_mutex);
        std::cout << line.str() << std::flush;
    } catch (...) {
        // Un fallo de consola no debe impedir que el Kernel entregue la respuesta.
    }
}

Response internal_server_error() {
    return Response{500, "application/json; charset=utf-8",
                    R"({"error":"internal_server_error"})"};
}
}

HttpKernel::HttpKernel(std::unique_ptr<Server> server) : server_(std::move(server)) {
    if (!server_) throw std::invalid_argument("HTTP kernel requires a server adapter");
}

HttpKernel::~HttpKernel() { stop(); }

void HttpKernel::use_global(Middleware middleware) {
    if (started_) throw std::logic_error("HTTP middleware is sealed after the kernel starts");
    if (!middleware) throw std::invalid_argument("Global HTTP middleware cannot be empty");
    global_middleware_.push_back(std::move(middleware));
}

void HttpKernel::use_api(Middleware middleware) {
    if (started_) throw std::logic_error("HTTP middleware is sealed after the kernel starts");
    if (!middleware) throw std::invalid_argument("API middleware cannot be empty");
    api_middleware_.push_back(std::move(middleware));
}

void HttpKernel::use_web(Middleware middleware) {
    if (started_) throw std::logic_error("HTTP middleware is sealed after the kernel starts");
    if (!middleware) throw std::invalid_argument("Web middleware cannot be empty");
    web_middleware_.push_back(std::move(middleware));
}

void HttpKernel::alias_middleware(std::string_view alias, Middleware middleware) {
    if (started_) throw std::logic_error("HTTP middleware aliases are sealed after the kernel starts");
    routes_.alias_middleware(alias, std::move(middleware));
}

void HttpKernel::configure_middleware(MiddlewareConfiguration configuration) {
    if (started_) throw std::logic_error("HTTP middleware is sealed after the kernel starts");

    // Validar callbacks y claves del bloque antes de modificar el Kernel.
    const auto validate_stack = [](const std::vector<Middleware>& stack) {
        for (const auto& middleware : stack) {
            if (!middleware) throw std::invalid_argument("Configured HTTP middleware cannot be empty");
        }
    };
    validate_stack(configuration.middleware);
    validate_stack(configuration.middleware_groups.api);
    validate_stack(configuration.middleware_groups.web);
    for (std::size_t i = 0; i < configuration.route_middleware.size(); ++i) {
        const auto& [alias, middleware] = configuration.route_middleware[i];
        if (alias.empty()) throw std::invalid_argument("HTTP route middleware alias cannot be empty");
        if (!middleware) throw std::invalid_argument("HTTP route middleware callback cannot be empty");
        for (std::size_t previous = 0; previous < i; ++previous) {
            if (configuration.route_middleware[previous].first == alias)
                throw std::logic_error("Duplicate HTTP middleware alias: " + alias);
        }
    }

    // Instalar aliases primero permite que el siguiente bloque valide rutas al registrarlas.
    for (auto& [alias, middleware] : configuration.route_middleware)
        alias_middleware(alias, std::move(middleware));
    for (auto& middleware : configuration.middleware) use_global(std::move(middleware));
    for (auto& middleware : configuration.middleware_groups.api) use_api(std::move(middleware));
    for (auto& middleware : configuration.middleware_groups.web) use_web(std::move(middleware));
}

void HttpKernel::set_exception_reporter(ExceptionReporter reporter) {
    if (started_) throw std::logic_error("HTTP exception reporter is sealed after the kernel starts");
    if (!reporter) throw std::invalid_argument("HTTP exception reporter cannot be empty");
    exception_reporter_ = std::move(reporter);
}

Response HttpKernel::handle(const RouteDefinition& route, const RequestView& request) const {
    using Clock = std::chrono::steady_clock;
    const auto received_at = std::chrono::system_clock::now();
    const auto started_at = Clock::now();
    Response response;
    try {
        if (!route.handler) throw std::invalid_argument("Route handler cannot be empty");

        const auto& group = route.kind == RouteKind::api ? api_middleware_ : web_middleware_;
        if (global_middleware_.empty() && group.empty() && route.middleware.empty()) {
            // Las rutas sin middleware toman el camino directo, sin construir la cadena.
            response = route.handler(request);
            response.status = normalized_status(response.status);
        } else {

            // Ejecutar las pilas sin copiar sus funciones por cada petición.
            const auto invoke_stack = [&](const auto& self, const std::vector<Middleware>& stack,
                                          std::size_t index, const Next& next) -> Response {
                if (index == stack.size()) return next();
                const Next continuation = [&self, &stack, index, &next] {
                    return self(self, stack, index + 1, next);
                };
                return stack[index](request, continuation);
            };

            const Next invoke_route_middleware = [&]() -> Response {
                const Next invoke_handler = [&]() { return route.handler(request); };
                return invoke_stack(invoke_stack, route.middleware, 0, invoke_handler);
            };
            const Next invoke_group_middleware = [&]() -> Response {
                return invoke_stack(invoke_stack, group, 0, invoke_route_middleware);
            };
            const Next next = invoke_group_middleware;
            response = invoke_stack(invoke_stack, global_middleware_, 0, next);
            response.status = normalized_status(response.status);
        }
    } catch (...) {
        // Una excepción del middleware o handler se convierte en un 500 genérico.
        // El reporte es opcional y no debe impedir que el cliente reciba la respuesta segura.
        if (exception_reporter_) {
            try { exception_reporter_(route, request, std::current_exception()); }
            catch (...) { /* Un logger fallido no debe escapar del límite HTTP. */ }
        }
        response = internal_server_error();
    }

    print_access_log(request, response.status, received_at, Clock::now() - started_at);
    return response;
}

void HttpKernel::run(const ServerSettings& settings, const ServerStartedCallback& started) {
    if (settings.host.empty()) throw std::invalid_argument("HTTP bind host cannot be empty");
    if (settings.port == 0) throw std::invalid_argument("HTTP port must be between 1 and 65535");
    if (started_) throw std::logic_error("HTTP kernel can only be started once");
    routes_.seal();
    started_ = true;
    server_->run(*this, settings, started);
}

void HttpKernel::stop() noexcept {
    if (server_) server_->stop();
}

} // namespace kotavel::http
