#include <kotavel/http/server.hpp>

#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

template<class Exception, class Function>
void expect_error(Function&& function, std::string_view expected,
                  const char* message) {
    try {
        function();
    } catch (const Exception& error) {
        check(std::string_view(error.what()).find(expected) != std::string_view::npos, message);
        return;
    } catch (...) {
        throw std::runtime_error(message);
    }
    throw std::runtime_error(message);
}

struct ServerState {
    bool stopped = false;
};

class FakeServer final : public kotavel::http::Server {
public:
    explicit FakeServer(std::shared_ptr<ServerState> state) : state_(std::move(state)) {}
    void run(const kotavel::http::HttpKernel&,
             const kotavel::http::ServerSettings&,
             const kotavel::http::ServerStartedCallback& started) override {
        if (started) started();
    }
    void stop() noexcept override { state_->stopped = true; }

private:
    std::shared_ptr<ServerState> state_;
};

// La configuración declarativa conserva global → grupo → aliases → handler.
void test_declarative_configuration_and_group_scopes() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(
        std::make_shared<ServerState>()));
    std::vector<std::string> order;
    int api_handler_calls = 0;
    int web_handler_calls = 0;

    kernel.configure_middleware({
        .middleware = {
            [&](const auto&, const auto& next) {
                order.emplace_back("global:before");
                auto response = next();
                order.emplace_back("global:after");
                return response;
            },
        },
        .middleware_groups = {
            .api = {
                [&](const auto&, const auto& next) {
                    order.emplace_back("api:before");
                    auto response = next();
                    order.emplace_back("api:after");
                    return response;
                },
            },
            .web = {
                [&](const auto&, const auto& next) {
                    order.emplace_back("web:before");
                    auto response = next();
                    order.emplace_back("web:after");
                    return response;
                },
            },
        },
        .route_middleware = {
            {"auth", [&](const auto&, const auto& next) {
                order.emplace_back("auth:before");
                auto response = next();
                order.emplace_back("auth:after");
                return response;
            }},
            {"company.context", [&](const auto&, const auto& next) {
                order.emplace_back("context:before");
                auto response = next();
                order.emplace_back("context:after");
                return response;
            }},
        },
    });

    auto api = kotavel::http::Route::api(kernel.routes());
    api.prefix("company").middleware({"auth", "company.context"}).group([&](auto& Route) {
        Route.get("/profile", [&](const auto&) {
            ++api_handler_calls;
            order.emplace_back("api:handler");
            return kotavel::http::Response{200, "text/plain", "api"};
        });
    });
    auto web = kotavel::http::Route::web(kernel.routes());
    web.get("/home", [&](const auto&) {
        ++web_handler_calls;
        order.emplace_back("web:handler");
        return kotavel::http::Response{200, "text/plain", "web"};
    }).middleware("auth");

    const auto api_response = kernel.handle(kernel.routes().routes()[0],
        {kotavel::http::Method::get, "/api/company/profile", {}});
    const std::vector<std::string> api_expected{
        "global:before", "api:before", "auth:before", "context:before", "api:handler",
        "context:after", "auth:after", "api:after", "global:after"};
    check(api_response.status == 200 && api_response.body == "api" && order == api_expected,
          "API requests should run global, API-group and route middleware in declaration order");
    check(api_handler_calls == 1 && web_handler_calls == 0,
          "the API request should invoke only its own route handler");

    order.clear();
    const auto web_response = kernel.handle(kernel.routes().routes()[1],
        {kotavel::http::Method::get, "/home", {}});
    const std::vector<std::string> web_expected{
        "global:before", "web:before", "auth:before", "web:handler", "auth:after",
        "web:after", "global:after"};
    check(web_response.status == 200 && web_response.body == "web" && order == web_expected,
          "web requests should use web defaults and exclude API defaults");
    check(api_handler_calls == 1 && web_handler_calls == 1,
          "each request should invoke its own route handler exactly once");
}

// Las claves y callbacks inválidos se rechazan durante la configuración.
void test_invalid_configuration_and_aliases() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(
        std::make_shared<ServerState>()));

    expect_error<std::invalid_argument>([&] {
        kernel.configure_middleware({.middleware = {kotavel::http::Middleware{}}});
    }, "cannot be empty", "empty global callbacks should be rejected");
    expect_error<std::invalid_argument>([&] {
        kernel.configure_middleware({
            .middleware_groups = {.api = {kotavel::http::Middleware{}}},
        });
    }, "cannot be empty", "empty API-group callbacks should be rejected");
    expect_error<std::invalid_argument>([&] {
        kernel.configure_middleware({
            .middleware_groups = {.web = {kotavel::http::Middleware{}}},
        });
    }, "cannot be empty", "empty web-group callbacks should be rejected");
    expect_error<std::invalid_argument>([&] {
        kernel.configure_middleware({
            .route_middleware = {{"", [](const auto&, const auto& next) { return next(); }}},
        });
    }, "cannot be empty", "empty route-middleware aliases should be rejected");
    expect_error<std::invalid_argument>([&] {
        kernel.configure_middleware({
            .route_middleware = {{"auth", kotavel::http::Middleware{}}},
        });
    }, "cannot be empty", "empty route-middleware callbacks should be rejected");
    expect_error<std::logic_error>([&] {
        kernel.configure_middleware({
            .route_middleware = {
                {"auth", [](const auto&, const auto& next) { return next(); }},
                {"auth", [](const auto&, const auto& next) { return next(); }},
            },
        });
    }, "Duplicate", "duplicate aliases in one block should be rejected");

    // Los errores anteriores no deben dejar una configuración parcialmente aplicada.
    kernel.configure_middleware({
        .route_middleware = {
            {"auth", [](const auto&, const auto& next) { return next(); }},
        },
    });
    expect_error<std::logic_error>([&] {
        kernel.configure_middleware({
            .route_middleware = {
                {"auth", [](const auto&, const auto& next) { return next(); }},
            },
        });
    }, "Duplicate", "aliases already registered on the kernel should remain unique");

    auto Route = kotavel::http::Route::api(kernel.routes());
    expect_error<std::invalid_argument>([&] {
        Route.get("/missing", [](const auto&) { return kotavel::http::Response{}; })
            .middleware("unknown.alias");
    }, "unknown.alias", "unknown aliases should fail while routes are configured");
    expect_error<std::invalid_argument>([&] {
        (void)Route.middleware(std::initializer_list<std::string_view>{});
    }, "cannot be empty", "empty alias lists should be rejected");
}

// El middleware puede cortar la cadena y sus excepciones no filtran detalles.
void test_short_circuit_and_exception_boundary() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(
        std::make_shared<ServerState>()));
    int handler_calls = 0;
    kernel.configure_middleware({
        .route_middleware = {
            {"deny", [](const auto&, const auto&) {
                return kotavel::http::Response{403, "text/plain", "denied"};
            }},
            {"throw", [](const auto&, const auto&) -> kotavel::http::Response {
                throw std::runtime_error("private middleware detail");
            }},
        },
    });
    auto Route = kotavel::http::Route::api(kernel.routes());
    Route.get("/denied", [&](const auto&) {
        ++handler_calls;
        return kotavel::http::Response{};
    }).middleware("deny");
    Route.get("/failure", [&](const auto&) {
        ++handler_calls;
        return kotavel::http::Response{};
    }).middleware("throw");

    const auto denied = kernel.handle(kernel.routes().routes()[0],
        {kotavel::http::Method::get, "/api/denied", {}});
    check(denied.status == 403 && denied.body == "denied" && handler_calls == 0,
          "middleware should stop the chain before invoking the handler");

    const auto failed = kernel.handle(kernel.routes().routes()[1],
        {kotavel::http::Method::get, "/api/failure", {}});
    check(failed.status == 500 &&
          failed.body == R"({"error":"internal_server_error"})" && handler_calls == 0,
          "middleware exceptions should become generic HTTP 500 responses");
}

// Las excepciones HTTP notifican al consumidor y siguen devolviendo un 500 seguro.
void test_unhandled_exception_reporter() {
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(
        std::make_shared<ServerState>()));
    int reports = 0;
    kernel.set_exception_reporter([&](const kotavel::http::RouteDefinition& route,
                                      const kotavel::http::RequestView& request,
                                      std::exception_ptr failure) {
        ++reports;
        check(route.path == "/api/failure" && request.path == "/api/failure",
              "exception reports should identify the route and request");
        try {
            std::rethrow_exception(failure);
        } catch (const std::runtime_error& error) {
            check(std::string_view(error.what()) == "handler failure",
                  "exception reports should preserve the original exception");
        }
    });
    kernel.routes().api_get("/failure", [](const auto&) -> kotavel::http::Response {
        throw std::runtime_error("handler failure");
    });

    const auto failed = kernel.handle(kernel.routes().routes().front(),
        {kotavel::http::Method::get, "/api/failure", {}});
    check(reports == 1 && failed.status == 500 &&
          failed.body == R"({"error":"internal_server_error"})",
          "the reporter should receive one exception without exposing it to the client");

    kotavel::http::HttpKernel reporter_failure(std::make_unique<FakeServer>(
        std::make_shared<ServerState>()));
    reporter_failure.set_exception_reporter([](const auto&, const auto&, auto) {
        throw std::runtime_error("logger failure");
    });
    reporter_failure.routes().api_get("/failure", [](const auto&) -> kotavel::http::Response {
        throw std::runtime_error("handler failure");
    });
    const auto safe_response = reporter_failure.handle(
        reporter_failure.routes().routes().front(),
        {kotavel::http::Method::get, "/api/failure", {}});
    check(safe_response.status == 500,
          "a failing reporter must not escape the HTTP exception boundary");

    expect_error<std::invalid_argument>([&] {
        reporter_failure.set_exception_reporter({});
    }, "cannot be empty", "an empty exception reporter should be rejected");
}

// El Kernel congela callbacks y aliases al arrancar el servidor.
void test_configuration_is_sealed_after_start() {
    auto state = std::make_shared<ServerState>();
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(state));
    kernel.configure_middleware({
        .middleware = {
            [](const auto&, const auto& next) { return next(); },
        },
    });
    kernel.routes().api_get("/health", [](const auto&) { return kotavel::http::Response{}; });
    kernel.run();

    expect_error<std::logic_error>([&] {
        kernel.configure_middleware({});
    }, "sealed", "declarative middleware config should be immutable after start");
    expect_error<std::logic_error>([&] {
        kernel.use_api([](const auto&, const auto& next) { return next(); });
    }, "sealed", "API defaults should be immutable after start");
    expect_error<std::logic_error>([&] {
        kernel.alias_middleware("late", [](const auto&, const auto& next) { return next(); });
    }, "sealed", "route aliases should be immutable after start");

    kernel.stop();
    check(state->stopped, "Kernel shutdown should reach the server adapter");
}
}

int main() {
    try {
        test_declarative_configuration_and_group_scopes();
        test_invalid_configuration_and_aliases();
        test_short_circuit_and_exception_boundary();
        test_unhandled_exception_reporter();
        test_configuration_is_sealed_after_start();
        std::cout << "HTTP middleware configuration and pipeline: OK\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
