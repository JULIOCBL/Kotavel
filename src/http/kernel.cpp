#include <kotavel/http/server.hpp>
#include <kotavel/http/api_response.hpp>

#include <algorithm>
#include <chrono>
#include <cerrno>
#include <cctype>
#include <cstdlib>
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

std::string_view trim(std::string_view value) noexcept {
    while (!value.empty() && std::isspace(static_cast<unsigned char>(value.front())))
        value.remove_prefix(1);
    while (!value.empty() && std::isspace(static_cast<unsigned char>(value.back())))
        value.remove_suffix(1);
    return value;
}

std::string lowercase(std::string_view value) {
    std::string result(value);
    for (char& character : result)
        character = static_cast<char>(std::tolower(static_cast<unsigned char>(character)));
    return result;
}

bool accepts_media_type(std::string_view accept, std::string_view offered) {
    accept = trim(accept);
    if (accept.empty()) return true;

    const auto offered_type = lowercase(trim(offered.substr(0, offered.find(';'))));
    const auto offered_slash = offered_type.find('/');
    if (offered_slash == std::string::npos) return true;
    const auto offered_major = offered_type.substr(0, offered_slash);

    int best_specificity = -1;
    double best_quality = 0.0;
    while (!accept.empty()) {
        const auto comma = accept.find(',');
        const auto member = trim(accept.substr(0, comma));
        const auto parameters = member.find(';');
        const auto range = lowercase(trim(member.substr(0, parameters)));
        double quality = 1.0;

        if (parameters != std::string_view::npos) {
            auto remaining = member.substr(parameters + 1);
            while (!remaining.empty()) {
                const auto separator = remaining.find(';');
                const auto parameter = trim(remaining.substr(0, separator));
                const auto equals = parameter.find('=');
                if (equals != std::string_view::npos &&
                    lowercase(trim(parameter.substr(0, equals))) == "q") {
                    const auto value = trim(parameter.substr(equals + 1));
                    const std::string value_string(value);
                    char* end = nullptr;
                    errno = 0;
                    const auto parsed = std::strtod(value_string.c_str(), &end);
                    quality = errno == 0 && end == value_string.c_str() + value_string.size() &&
                                      parsed >= 0.0 && parsed <= 1.0
                                  ? parsed : 0.0;
                    break;
                }
                if (separator == std::string_view::npos) break;
                remaining.remove_prefix(separator + 1);
            }
        }

        int specificity = -1;
        if (range == offered_type) specificity = 2;
        else if (range == offered_major + "/*") specificity = 1;
        else if (range == "*/*") specificity = 0;
        if (specificity > best_specificity) {
            best_specificity = specificity;
            best_quality = quality;
        } else if (specificity == best_specificity && specificity >= 0) {
            best_quality = std::max(best_quality, quality);
        }

        if (comma == std::string_view::npos) break;
        accept.remove_prefix(comma + 1);
        accept = trim(accept);
    }
    return best_specificity >= 0 && best_quality > 0.0;
}

std::string_view default_media_type(RouteKind kind) noexcept {
    return kind == RouteKind::api ? "application/json" : "text/html";
}

bool request_accepts_route(const Request& request, RouteKind kind) {
    const auto accept = request.header("accept");
    return accepts_media_type(accept.value_or(std::string_view{}), default_media_type(kind));
}

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

void print_access_log(const Request& request, int status,
                      std::chrono::system_clock::time_point received_at,
                      std::chrono::steady_clock::duration elapsed) noexcept {
    try {
        const auto timestamp = std::chrono::floor<std::chrono::milliseconds>(received_at);
        const auto whole_seconds = std::chrono::floor<std::chrono::seconds>(timestamp);
        const auto milliseconds_part =
            std::chrono::duration_cast<std::chrono::milliseconds>(timestamp - whole_seconds).count();
        const auto time_value = std::chrono::system_clock::to_time_t(received_at);
        std::tm utc_time{};
#if defined(_WIN32)
        const bool has_utc_time = gmtime_s(&utc_time, &time_value) == 0;
#else
        const bool has_utc_time = gmtime_r(&time_value, &utc_time) != nullptr;
#endif

        // Sanitizar controles evita que una ruta pueda falsificar líneas en la terminal.
        std::string printable_path(request.path());
        for (char& character : printable_path) {
            if (character == '\r' || character == '\n' || character == '\t') character = ' ';
        }

        const auto elapsed_ms = std::chrono::duration<double, std::milli>(elapsed).count();
        std::ostringstream line;
        line << "[HTTP] ";
        if (has_utc_time) {
            line << std::put_time(&utc_time, "%Y-%m-%dT%H:%M:%S") << '.'
                 << std::setw(3) << std::setfill('0') << milliseconds_part << 'Z';
        } else {
            line << "unknown-time";
        }
        const auto method = request.raw_method().empty()
            ? method_name(request.method()) : request.raw_method();
        line << ' ' << method << ' ' << printable_path << ' '
             << normalized_status(status) << ' ' << std::fixed << std::setprecision(3)
             << elapsed_ms << " ms\n";

        // El servidor puede atender en paralelo; el lock conserva cada línea completa.
        const std::lock_guard lock(access_log_mutex);
        std::cout << line.str() << std::flush;
    } catch (...) {
        // Un fallo de consola no debe impedir que el Kernel entregue la respuesta.
    }
}

Response internal_server_error(RouteKind kind) {
    if (kind == RouteKind::api)
        return ApiResponse::error(500, "Internal Server Error",
                                  "An unexpected error occurred while processing the request.");
    return Response{500, "text/html; charset=utf-8",
                    "<!doctype html><html><head><title>500 Internal Server Error</title></head>"
                    "<body><h1>500 Internal Server Error</h1></body></html>"};
}

Response invalid_json_request(RouteKind kind) {
    if (kind == RouteKind::api)
        return ApiResponse::error(400, "Bad Request", "The request body contains invalid JSON.");
    return Response{400, "text/html; charset=utf-8",
                    "<!doctype html><html><head><title>400 Bad Request</title></head>"
                    "<body><h1>400 Bad Request</h1></body></html>"};
}

Response not_acceptable(RouteKind kind) {
    if (kind == RouteKind::api)
        return ApiResponse::error(406, "Not Acceptable",
                                  "This route does not provide a representation accepted by the request.");
    return Response{406, "text/html; charset=utf-8",
                    "<!doctype html><html><head><title>406 Not Acceptable</title></head>"
                    "<body><h1>406 Not Acceptable</h1></body></html>"};
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

Response HttpKernel::handle(const RouteDefinition& route, const Request& request) const {
    const detail::RequestScope request_scope(request);
    // Negotiate before middleware so an unsupported representation cannot trigger
    // side effects such as writes, authentication challenges, or controller work.
    if (!request_accepts_route(request, route.kind)) return not_acceptable(route.kind);
    Response response;
    try {
        if (!route.handler) throw std::invalid_argument("Route handler cannot be empty");
        if (!request.has_valid_input()) return invalid_json_request(route.kind);

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
        response = internal_server_error(route.kind);
    }

    return response;
}

Response HttpKernel::not_found(const Request& request) const {
    const auto kind = request.path() == "/api" || request.path().starts_with("/api/")
        ? RouteKind::api : RouteKind::web;
    if (!request_accepts_route(request, kind)) return not_acceptable(kind);
    if (kind == RouteKind::api)
        return ApiResponse::error(404, "Not Found", "The requested API route was not found.");
    return Response{404, "text/html; charset=utf-8",
                    "<!doctype html><html><head><title>404 Not Found</title></head>"
                    "<body><h1>404 Not Found</h1></body></html>"};
}

Response HttpKernel::method_not_allowed(const Request& request) const {
    const auto kind = request.kind();
    if (!request_accepts_route(request, kind)) return not_acceptable(kind);

    const bool options = lowercase(request.raw_method()) == "options";
    const int status = options ? 403 : 405;
    const auto title = options ? "Forbidden" : "Method Not Allowed";
    const auto detail = options
        ? "This HTTP method is not permitted for the requested route."
        : "The requested HTTP method is not allowed for this route.";
    if (kind == RouteKind::api) return ApiResponse::error(status, title, detail);

    const auto heading = std::to_string(status) + " " + title;
    return Response{status, "text/html; charset=utf-8",
                    "<!doctype html><html><head><title>" + heading +
                    "</title></head><body><h1>" + heading + "</h1></body></html>"};
}

void HttpKernel::log_access(const Request& request, int status,
                            std::chrono::system_clock::time_point received_at,
                            std::chrono::steady_clock::duration elapsed) const noexcept {
    print_access_log(request, status, received_at, elapsed);
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
