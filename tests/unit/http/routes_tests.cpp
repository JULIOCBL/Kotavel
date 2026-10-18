#include <kotavel/http/server.hpp>
#include <kotavel/http/api_response.hpp>
#include <kotavel/http/controller.hpp>

#include <algorithm>
#include <array>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

class StoreControllerRequest final : public kotavel::http::FormRequest {
public:
    using FormRequest::FormRequest;
    kotavel::http::ValidationRules rules() const override {
        return {{"name", {"required", "string", "min:3"}}};
    }
};

class RouteBoundController final : public kotavel::http::Controller {
public:
    kotavel::http::Response index(const kotavel::http::Request& request) const {
        return {200, "text/plain", std::string(request.path())};
    }
    kotavel::http::Response show(const kotavel::http::Request& request) const {
        return {200, "text/plain", std::string(request.route("id").value_or("missing"))};
    }
    kotavel::http::Response store(const StoreControllerRequest& request) const {
        return kotavel::http::ApiResponse::success(request.validated(), 201);
    }
};

void test_controller_member_routes() {
    kotavel::http::Router routes;
    RouteBoundController controller;
    auto items = kotavel::http::Route::api(routes).prefix("items").controller(controller);
    items.get("/", &RouteBoundController::index).name("api.items.index");
    items.group([](auto& items) {
        items.post("/", &RouteBoundController::store).name("api.items.store");
        items.prefix("{id}").group([](auto& item) {
            item.get("/", &RouteBoundController::show).name("api.items.show");
        });
    });

    const auto& index = routes.routes()[0];
    const auto index_response = index.handler(
        kotavel::http::Request(kotavel::http::Method::get, "/api/items/", {}));
    check(index.path == "/api/items" && index_response.status == 200 &&
          index_response.body == "/api/items/",
          "controller member pointers should preserve route prefixes and receive Request");

    const auto& store = routes.routes()[1];
    const auto valid_request = kotavel::http::Request(kotavel::http::RequestData{
        .method = kotavel::http::Method::post,
        .kind = kotavel::http::RouteKind::api,
        .path = "/api/items/",
        .body = R"({"name":"Kotavel"})",
        .headers = {{"content-type", "application/json"}},
    });
    const auto created = store.handler(valid_request);
    check(created.status == 201 && created.body.find("Kotavel") != std::string::npos,
          "controller member pointers should validate FormRequest before invoking the action");

    const auto invalid_request = kotavel::http::Request(kotavel::http::RequestData{
        .method = kotavel::http::Method::post,
        .kind = kotavel::http::RouteKind::api,
        .path = "/api/items/",
        .body = "{}",
        .headers = {{"content-type", "application/json"}},
    });
    const auto rejected = store.handler(invalid_request);
    check(rejected.status == 422,
          "controller member pointers should return FormRequest errors before action execution");

    const auto& show = routes.routes()[2];
    const auto show_request = kotavel::http::Request(kotavel::http::RequestData{
        .method = kotavel::http::Method::get,
        .kind = kotavel::http::RouteKind::api,
        .path = "/api/items/42",
        .route_parameters = {{"id", "42"}},
    });
    const auto shown = show.handler(show_request);
    check(show.path == "/api/items/{id}" && shown.body == "42",
          "controller route groups should retain nested prefixes and route parameters");
}

template<class Exception, class Function>
void expect(Function&& function, const char* message) {
    try { function(); }
    catch (const Exception&) { return; }
    throw std::runtime_error(message);
}

template<class Exception, class Function>
void expect_error(Function&& function, std::string_view expected_text, const char* message) {
    try { function(); }
    catch (const Exception& error) {
        check(std::string_view(error.what()).find(expected_text) != std::string_view::npos, message);
        return;
    }
    catch (...) { throw std::runtime_error(message); }
    throw std::runtime_error(message);
}

struct FakeServerState {
    bool called = false;
    bool stopped = false;
    std::size_t route_count = 0;
    std::string host;
    std::uint16_t port = 0;
};

class FakeServer final : public kotavel::http::Server {
public:
    explicit FakeServer(std::shared_ptr<FakeServerState> state) : state_(std::move(state)) {}
    void run(const kotavel::http::HttpKernel& kernel,
             const kotavel::http::ServerSettings& settings,
             const kotavel::http::ServerStartedCallback& started) override {
        state_->called = true;
        state_->route_count = kernel.routes().routes().size();
        state_->host = settings.host;
        state_->port = settings.port;
        if (started) started();
    }
    void stop() noexcept override { state_->stopped = true; }

private:
    std::shared_ptr<FakeServerState> state_;
};

// La clasificación API/web conserva prefijos y rechaza conflictos al arrancar.
void test_http_route_groups() {
    kotavel::http::Router routes;
    routes.api_get("/health", [](const kotavel::http::RequestView&) {
        return kotavel::http::Response{200, "application/json", R"({"status":"ok"})"};
    });
    routes.web_get("/", [](const kotavel::http::RequestView&) {
        return kotavel::http::Response{200, "text/html", "<h1>Kotavel</h1>"};
    });

    check(routes.routes().size() == 2, "API and web routes should both be registered");
    check(routes.routes()[0].path == "/api/health", "API routes should receive the /api prefix");
    check(routes.routes()[0].kind == kotavel::http::RouteKind::api, "API route kind should be preserved");
    check(routes.routes()[1].path == "/", "web routes should preserve their paths");
    check(routes.routes()[1].kind == kotavel::http::RouteKind::web, "web route kind should be preserved");
    check(routes.routes()[0].handler({kotavel::http::Method::get, "/api/health", {}}).status == 200,
          "registered handlers should be callable without a transport");

    expect<std::logic_error>([&] {
        routes.api_get("/health", [](const kotavel::http::RequestView&) {
            return kotavel::http::Response{};
        });
    }, "duplicate method/path pairs must fail");
    expect<std::invalid_argument>([&] {
        routes.web_get("relative", [](const kotavel::http::RequestView&) {
            return kotavel::http::Response{};
        });
    }, "relative route paths must fail");
}

// El DSL mantiene el estilo Route de Laravel y el Router explícito por app.
void test_laravel_style_route_dsl() {
    kotavel::http::Router routes;
    auto Route = kotavel::http::Route::api(routes);
    Route.get("/health", [](const kotavel::http::RequestView&) {
        return kotavel::http::Response{200, "application/json", "{}"};
    });
    Route.prefix("v1").group([](auto& Route) {
        Route.get("users", [](const kotavel::http::RequestView&) {
            return kotavel::http::Response{};
        });
        Route.prefix("users").group([](auto& Route) {
            Route.post("/", [](const kotavel::http::RequestView&) {
                return kotavel::http::Response{};
            });
            Route.put("/1", [](const kotavel::http::RequestView&) {
                return kotavel::http::Response{};
            });
            Route.patch("/1", [](const kotavel::http::RequestView&) {
                return kotavel::http::Response{};
            });
            Route.remove("/1", [](const kotavel::http::RequestView&) {
                return kotavel::http::Response{};
            });
        });
    });

    check(routes.routes().size() == 6, "route facade should register every HTTP method");
    check(routes.routes()[0].path == "/api/health", "API facade should retain the /api prefix");
    check(routes.routes()[1].path == "/api/v1/users", "prefix groups should accept relative paths");
    check(routes.routes()[2].path == "/api/v1/users", "root paths should resolve to the group prefix");
    check(routes.routes()[3].path == "/api/v1/users/1", "nested groups should combine prefixes");

    auto Web = kotavel::http::Route::web(routes);
    Web.prefix("admin/").group([](auto& Route) {
        Route.get("/users", [](const kotavel::http::RequestView&) {
            return kotavel::http::Response{};
        });
    });
    check(routes.routes().back().path == "/admin/users" &&
          routes.routes().back().kind == kotavel::http::RouteKind::web,
          "web facade should preserve group prefix without adding /api");
    expect<std::invalid_argument>([&] { (void)Route.prefix("/"); },
          "root-only route prefixes must fail clearly");
}

// Los nombres son únicos y permiten obtener la URL registrada sin buscarla a mano.
void test_named_routes_and_url_generation() {
    kotavel::http::Router routes;
    auto api = kotavel::http::Route::api(routes);
    api.get("/health", [](const auto&) { return kotavel::http::Response{}; })
        .name("api.health");
    api.prefix("v1").get("/users", [](const auto&) {
        return kotavel::http::Response{};
    }).name("api.v1.users.index");
    auto web = kotavel::http::Route::web(routes);
    web.get("/", [](const auto&) { return kotavel::http::Response{}; })
        .name("web.home");

    check(routes.routes()[0].name == "api.health" &&
          routes.url_for("api.health") == "/api/health",
          "named API routes should expose their name and resolved URL");
    check(routes.url_for("api.v1.users.index") == "/api/v1/users",
          "named routes should retain nested API prefixes");
    check(routes.url_for("web.home") == "/",
          "named web routes should resolve without an API prefix");

    expect_error<std::logic_error>([&] {
        web.get("/duplicate-name", [](const auto&) { return kotavel::http::Response{}; })
            .name("api.health");
    }, "Duplicate HTTP route name", "route names must be unique across API and web");
    expect_error<std::invalid_argument>([&] {
        web.get("/empty-name", [](const auto&) { return kotavel::http::Response{}; }).name("");
    }, "cannot be empty", "empty route names should be rejected");
    expect_error<std::out_of_range>([&] { (void)routes.url_for("missing.route"); },
        "Named HTTP route not found", "URL generation should clearly report unknown names");
    expect_error<std::logic_error>([&] { (void)api.name("not-an-endpoint"); },
        "must follow", "a name cannot be attached to a group without a registered endpoint");

    // Reordenar name() y middleware() en el mismo endpoint no cambia su efecto.
    auto state = std::make_shared<FakeServerState>();
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(state));
    auto endpoints = kotavel::http::Route::api(kernel.routes());
    const auto deny_blocked_payload = [](const auto& request,
                                         const kotavel::http::Next& next) {
        if (request.body() == "blocked")
            return kotavel::http::Response{403, "text/plain", "forbidden"};
        return next();
    };
    const auto handler = [](const auto&) {
        return kotavel::http::Response{200, "text/plain", "ok"};
    };
    endpoints.get("/name-first", handler)
        .name("api.name_first")
        .middleware(deny_blocked_payload);
    endpoints.get("/middleware-first", handler)
        .middleware(deny_blocked_payload)
        .name("api.middleware_first");

    const auto& name_first = kernel.routes().routes()[0];
    const auto& middleware_first = kernel.routes().routes()[1];
    check(name_first.middleware.size() == 1 && middleware_first.middleware.size() == 1 &&
          kernel.routes().url_for("api.name_first") == "/api/name-first" &&
          kernel.routes().url_for("api.middleware_first") == "/api/middleware-first",
          "name and endpoint middleware should compose in either declaration order");
    const auto rejected_first = kernel.handle(name_first,
        {kotavel::http::Method::get, "/api/name-first", "blocked"});
    const auto rejected_second = kernel.handle(middleware_first,
        {kotavel::http::Method::get, "/api/middleware-first", "blocked"});
    check(rejected_first.status == 403 && rejected_second.status == 403,
          "reordering endpoint modifiers must preserve middleware behavior");
}

// Todos los verbos conservan método, grupo, prefijo y vista de solicitud.
void test_every_http_verb_and_request_view() {
    kotavel::http::Router routes;
    auto api = kotavel::http::Route::api(routes);
    auto web = kotavel::http::Route::web(routes);
    const auto handler = [](const kotavel::http::RequestView& request) {
        return kotavel::http::Response{
            200, "text/plain", std::string(request.path()) + ":" + std::string(request.body())};
    };

    api.get("/resource", handler);
    api.post("/resource", handler);
    api.put("/resource", handler);
    api.patch("/resource", handler);
    api.remove("/resource", handler);
    web.get("/resource", handler);
    web.post("/resource", handler);
    web.put("/resource", handler);
    web.patch("/resource", handler);
    web.remove("/resource", handler);

    constexpr std::array methods{
        kotavel::http::Method::get, kotavel::http::Method::post,
        kotavel::http::Method::put, kotavel::http::Method::patch,
        kotavel::http::Method::delete_};
    check(routes.routes().size() == methods.size() * 2,
          "API and web should register all five supported HTTP methods");
    for (std::size_t i = 0; i < methods.size(); ++i) {
        const auto& api_route = routes.routes()[i];
        const auto& web_route = routes.routes()[i + methods.size()];
        check(api_route.method == methods[i] && api_route.kind == kotavel::http::RouteKind::api &&
              api_route.path == "/api/resource",
              "API route table should preserve each method and apply /api exactly once");
        check(web_route.method == methods[i] && web_route.kind == kotavel::http::RouteKind::web &&
              web_route.path == "/resource",
              "web route table should preserve each method without the API prefix");

        const kotavel::http::RequestView request{methods[i], api_route.path, "payload"};
        const auto response = api_route.handler(request);
        check(response.status == 200 && response.body == "/api/resource:payload",
              "route handlers should receive a valid request view and return their response");
    }
}

// El Kernel aplica primero middleware global, luego grupo y después la ruta.
void test_http_kernel_middleware_pipeline() {
    auto state = std::make_shared<FakeServerState>();
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(state));
    std::vector<std::string> order;
    bool handler_called = false;

    kernel.use_global([&](const auto&, const kotavel::http::Next& next) {
        order.push_back("global:before");
        auto response = next();
        order.push_back("global:after");
        return response;
    });
    kernel.use_api([&](const auto&, const kotavel::http::Next& next) {
        order.push_back("api:before");
        auto response = next();
        order.push_back("api:after");
        return response;
    });
    kernel.use_web([&](const auto&, const kotavel::http::Next& next) {
        order.push_back("web:before");
        return next();
    });

    auto api_routes = kotavel::http::Route::api(kernel.routes());
    api_routes.prefix("guarded").middleware([&](const auto&, const kotavel::http::Next& next) {
        order.push_back("route:before");
        auto response = next();
        order.push_back("route:after");
        return response;
    }).get("/pipeline", [&](const auto&) {
        handler_called = true;
        order.push_back("handler");
        return kotavel::http::Response{202, "text/plain", "ok"};
    });
    api_routes.get("/blocked", [&](const auto&) {
        handler_called = true;
        return kotavel::http::Response{200, "text/plain", "unexpected"};
    });
    api_routes.get("/single", [&](const auto&) {
        handler_called = true;
        return kotavel::http::Response{200, "text/plain", "unexpected"};
    }).middleware([](const auto&, const kotavel::http::Next&) {
        return kotavel::http::Response{403, "text/plain", "forbidden"};
    }).name("api.single");
    auto web_routes = kotavel::http::Route::web(kernel.routes());
    web_routes.get("/home", [](const auto&) {
        return kotavel::http::Response{200, "text/plain", "web"};
    });

    const auto& pipeline_route = kernel.routes().routes()[0];
    const auto pipeline_response = kernel.handle(pipeline_route,
        {kotavel::http::Method::get, "/api/guarded/pipeline", {}});
    check(pipeline_response.status == 202 && pipeline_response.body == "ok",
          "the middleware pipeline should preserve the handler response");
    const std::vector<std::string> expected_order{
        "global:before", "api:before", "route:before", "handler",
        "route:after", "api:after", "global:after"};
    check(order == expected_order,
          "global, API-group and route middleware should wrap the handler in order");

    order.clear();
    handler_called = false;
    const auto& blocked_route = kernel.routes().routes()[1];
    const auto blocked_response = kernel.handle(blocked_route,
        {kotavel::http::Method::get, "/api/blocked", {}});
    check(blocked_response.status == 200 && handler_called,
          "middleware should continue when it does not short-circuit");

    order.clear();
    handler_called = false;
    const auto& single_route = kernel.routes().routes()[2];
    const auto single_response = kernel.handle(single_route,
        {kotavel::http::Method::get, "/api/single", {}});
    check(single_response.status == 403 && !handler_called,
          "middleware chained after a verb should affect only that endpoint");

    order.clear();
    const auto& web_route = kernel.routes().routes()[3];
    const auto web_response = kernel.handle(web_route,
        {kotavel::http::Method::get, "/home", {}});
    check(web_response.body == "web" && order.front() == "global:before" &&
          std::find(order.begin(), order.end(), "web:before") != order.end() &&
          std::find(order.begin(), order.end(), "api:before") == order.end(),
          "web routes should receive web middleware and not the API group");

    kotavel::http::HttpKernel short_circuit(std::make_unique<FakeServer>(state));
    int endpoint_calls = 0;
    short_circuit.use_global([](const auto&, const kotavel::http::Next&) {
        return kotavel::http::Response{429, "text/plain", "limited"};
    });
    short_circuit.routes().api_get("/limited", [&](const auto&) {
        ++endpoint_calls;
        return kotavel::http::Response{};
    });
    const auto denied = short_circuit.handle(short_circuit.routes().routes().front(),
        {kotavel::http::Method::get, "/api/limited", {}});
    check(denied.status == 429 && endpoint_calls == 0,
          "middleware should be able to return a response without invoking the route");

    kotavel::http::HttpKernel failure(std::make_unique<FakeServer>(state));
    failure.use_api([](const auto&, const kotavel::http::Next&) -> kotavel::http::Response {
        throw std::runtime_error("private middleware failure");
    });
    failure.routes().api_get("/failure", [](const auto&) {
        return kotavel::http::Response{200, "text/plain", "unreachable"};
    });
    const auto failed = failure.handle(failure.routes().routes().front(),
        {kotavel::http::Method::get, "/api/failure", {}});
    check(failed.status == 500 && failed.body ==
          R"({"errors":[{"detail":"An unexpected error occurred while processing the request.","status":"500","title":"Internal Server Error"}]})",
          "middleware exceptions should become a generic HTTP 500 response");

    kotavel::http::HttpKernel web_failure(std::make_unique<FakeServer>(state));
    web_failure.routes().web_get("/failure", [](const auto&) -> kotavel::http::Response {
        throw std::runtime_error("private web failure");
    });
    const auto web_failed = web_failure.handle(web_failure.routes().routes().front(),
        {kotavel::http::Method::get, "/failure", {}});
    check(web_failed.status == 500 && web_failed.content_type == "text/html; charset=utf-8" &&
          web_failed.body.find("500 Internal Server Error") != std::string::npos &&
          web_failed.body.find("private web failure") == std::string::npos,
          "web exceptions should remain HTML and hide internal exception details");

    expect<std::invalid_argument>([&] { kernel.use_global({}); },
          "empty global middleware must be rejected");
    expect<std::invalid_argument>([&] { kernel.use_api({}); },
          "empty API middleware must be rejected");
    expect<std::invalid_argument>([&] { kernel.use_web({}); },
          "empty web middleware must be rejected");
}

// Los alias de texto se resuelven durante el registro y preservan el orden declarado.
void test_string_middleware_aliases() {
    auto state = std::make_shared<FakeServerState>();
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(state));
    std::vector<std::string> order;
    int handler_calls = 0;

    kernel.configure_middleware({
        .middleware = {
            [&](const auto&, const auto& next) {
                order.push_back("global:before");
                auto response = next();
                order.push_back("global:after");
                return response;
            },
        },
        .middleware_groups = {
            .api = {
                [&](const auto&, const auto& next) {
                    order.push_back("api:before");
                    auto response = next();
                    order.push_back("api:after");
                    return response;
                },
            },
            .web = {},
        },
        .route_middleware = {
            {"ensure.auth", [&](const auto&, const auto& next) {
                order.push_back("auth:before");
                auto response = next();
                order.push_back("auth:after");
                return response;
            }},
            {"company.billing", [&](const auto&, const auto& next) {
                order.push_back("billing:before");
                auto response = next();
                order.push_back("billing:after");
                return response;
            }},
            {"company.context", [&](const auto&, const auto& next) {
                order.push_back("context:before");
                auto response = next();
                order.push_back("context:after");
                return response;
            }},
        },
    });

    auto api = kotavel::http::Route::api(kernel.routes());
    api.prefix("company")
        .middleware({"ensure.auth", "company.billing"})
        .group([&](auto& Route) {
            Route.get("/profile", [&](const auto&) {
                ++handler_calls;
                order.push_back("profile");
                return kotavel::http::Response{200, "text/plain", "profile"};
            }).name("company.profile");
        });
    api.get("/context", [&](const auto&) {
        ++handler_calls;
        order.push_back("context-handler");
        return kotavel::http::Response{200, "text/plain", "context"};
    }).middleware("company.context").name("api.context");

    check(kernel.routes().routes().size() == 2 &&
          kernel.routes().url_for("company.profile") == "/api/company/profile",
          "string middleware aliases should compose with route groups and named routes");
    const auto grouped = kernel.handle(kernel.routes().routes()[0],
        {kotavel::http::Method::get, "/api/company/profile", {}});
    const std::vector<std::string> expected_group_order{
        "global:before", "api:before", "auth:before", "billing:before", "profile",
        "billing:after", "auth:after", "api:after", "global:after"};
    check(grouped.status == 200 && grouped.body == "profile" && order == expected_group_order,
          "a middleware alias list on a group should run in declaration order");

    order.clear();
    const auto individual = kernel.handle(kernel.routes().routes()[1],
        {kotavel::http::Method::get, "/api/context", {}});
    const std::vector<std::string> expected_individual_order{
        "global:before", "api:before", "context:before", "context-handler",
        "context:after", "api:after", "global:after"};
    check(individual.status == 200 && order == expected_individual_order && handler_calls == 2,
          "a single string alias should affect only its endpoint");

    expect_error<std::invalid_argument>([&] {
        (void)api.get("/unknown", [](const auto&) { return kotavel::http::Response{}; })
            .middleware("missing.alias");
    }, "missing.alias", "unknown middleware aliases should fail during route configuration");
    expect_error<std::logic_error>([&] {
        kernel.alias_middleware("ensure.auth", [](const auto&, const auto& next) {
            return next();
        });
    }, "Duplicate", "middleware aliases should be unique");
    expect_error<std::invalid_argument>([&] {
        kernel.alias_middleware("", [](const auto&, const auto& next) { return next(); });
    }, "cannot be empty", "empty middleware aliases should be rejected");
    expect_error<std::invalid_argument>([&] {
        (void)api.middleware(std::initializer_list<std::string_view>{});
    }, "cannot be empty", "empty middleware alias lists should be rejected");

    kernel.run();
    expect_error<std::logic_error>([&] {
        kernel.alias_middleware("late.alias", [](const auto&, const auto& next) {
            return next();
        });
    }, "sealed", "middleware aliases must not change after the kernel starts");
}

// Entradas inválidas deben fallar al registrar rutas, no al atender una petición.
void test_route_registration_validation_and_conflicts() {
    kotavel::http::Router routes;
    auto Route = kotavel::http::Route::api(routes);

    expect_error<std::invalid_argument>([&] {
        routes.web_get("", [](const auto&) { return kotavel::http::Response{}; });
    }, "Route path", "the low-level router should reject empty paths");
    expect_error<std::invalid_argument>([&] { Route.get("/items?active=1", [](const auto&) { return kotavel::http::Response{}; }); },
        "query", "query strings should not be accepted as route definitions");
    expect_error<std::invalid_argument>([&] { Route.get("/items#section", [](const auto&) { return kotavel::http::Response{}; }); },
        "fragment", "URI fragments should not be accepted as route definitions");
    expect_error<std::invalid_argument>([&] { Route.get("/items", {}); },
        "handler", "empty handlers should report the missing callback");
    expect_error<std::invalid_argument>([&] { (void)Route.prefix(""); },
        "prefix", "empty group prefixes should fail immediately");
    expect_error<std::invalid_argument>([&] { Route.group({}); },
        "group", "empty groups should report the missing callback");

    Route.get("/items", [](const auto&) { return kotavel::http::Response{}; });
    expect_error<std::logic_error>([&] {
        Route.get("/api/items", [](const auto&) { return kotavel::http::Response{}; });
    }, "Duplicate", "explicit and automatic /api prefixes should detect duplicate routes");

    auto Web = kotavel::http::Route::web(routes);
    expect_error<std::logic_error>([&] {
        Web.get("/api/items", [](const auto&) { return kotavel::http::Response{}; });
    }, "Duplicate", "API and web declarations for the same method and URL should conflict");
}

// Registrar anidado conserva aislamiento del padre y del contexto de la aplicación.
void test_nested_route_scopes_and_sealing() {
    kotavel::http::Router routes;
    auto api = kotavel::http::Route::api(routes);
    auto nested = api.prefix("v1").prefix("users/");
    nested.group([](auto& Route) {
        Route.get("", [](const auto&) { return kotavel::http::Response{}; });
        Route.get("/42/profile", [](const auto&) { return kotavel::http::Response{}; });
    });
    api.get("/health", [](const auto&) { return kotavel::http::Response{}; });

    check(routes.routes().size() == 3, "group registration should not lose sibling routes");
    check(routes.routes()[0].path == "/api/v1/users" &&
          routes.routes()[1].path == "/api/v1/users/42/profile" &&
          routes.routes()[2].path == "/api/health",
          "nested prefix scopes should combine cleanly and leave the parent unchanged");

    routes.seal();
    expect_error<std::logic_error>([&] {
        nested.get("/late", [](const auto&) { return kotavel::http::Response{}; });
    }, "sealed", "a retained registrar must respect the router sealed state");
}

// El Kernel conserva todos los estados HTTP de error válidos y normaliza valores inválidos.
void test_http_error_status_responses() {
    auto state = std::make_shared<FakeServerState>();
    kotavel::http::HttpKernel kernel(std::make_unique<FakeServer>(state));
    int status = 400;
    kernel.routes().api_get("/error", [&](const auto&) {
        return kotavel::http::Response{status, "application/json", R"({"error":"test"})"};
    });

    const auto& route = kernel.routes().routes().front();
    const kotavel::http::RequestView request{
        kotavel::http::Method::get, "/api/error", {}};
    for (int error_status = 400; error_status <= 599; ++error_status) {
        status = error_status;
        const auto response = kernel.handle(route, request);
        check(response.status == error_status &&
              response.content_type == "application/json" &&
              response.body == R"({"error":"test"})",
              "the Kernel should preserve every valid 4xx and 5xx response status and body");
    }

    for (const int invalid_status : {99, 600}) {
        status = invalid_status;
        const auto response = kernel.handle(route, request);
        check(response.status == 500,
              "out-of-range HTTP statuses should be normalized to HTTP 500");
    }

    const auto missing = kernel.not_found(
        {kotavel::http::Method::get, "/api/missing", {}});
    check(missing.status == 404 &&
          missing.content_type == "application/json; charset=utf-8" &&
          missing.body ==
          R"({"errors":[{"detail":"The requested API route was not found.","status":"404","title":"Not Found"}]})",
          "the Kernel should use the public API error format for unknown API paths");

    const auto missing_web = kernel.not_found(
        {kotavel::http::Method::get, "/missing", {}});
    check(missing_web.status == 404 && missing_web.content_type == "text/html; charset=utf-8" &&
          missing_web.body.find("404 Not Found") != std::string::npos,
          "unknown web paths should receive an HTML error page");
}

// El kernel valida el listener y entrega la tabla de rutas al adaptador.
void test_http_kernel_defaults_and_validation() {
    auto observed = std::make_shared<FakeServerState>();
    auto adapter = std::make_unique<FakeServer>(observed);
    {
        kotavel::http::HttpKernel kernel(std::move(adapter));
        kernel.routes().api_get("/health", [](const kotavel::http::RequestView&) {
            return kotavel::http::Response{};
        });
        const kotavel::http::ServerSettings defaults;
        check(defaults.host == "0.0.0.0" && defaults.port == 8080,
              "HTTP defaults should accept local and network connections");
        bool server_started = false;
        kernel.run({}, [&] { server_started = true; });
        check(observed->called && observed->route_count == 1,
              "kernel should run the configured adapter with registered routes");
        check(server_started, "kernel should forward its transport-ready callback");
        check(observed->host == "0.0.0.0" && observed->port == 8080,
              "kernel should pass the default listener settings");
        check(kernel.routes().sealed(), "routes should be sealed before accepting requests");
        expect<std::logic_error>([&] {
            kernel.routes().web_get("/late", [](const kotavel::http::RequestView&) {
                return kotavel::http::Response{};
            });
        }, "routes must not change after the kernel starts");
        expect<std::logic_error>([&] { kernel.run(); }, "kernel restart must be rejected");
        expect<std::logic_error>([&] {
            kernel.use_global([](const auto&, const kotavel::http::Next& next) {
                return next();
            });
        }, "middleware registration must be rejected after the kernel starts");
    }
    check(observed->stopped, "kernel destruction should request adapter shutdown");

    auto invalid_adapter = std::make_unique<FakeServer>(std::make_shared<FakeServerState>());
    kotavel::http::HttpKernel kernel(std::move(invalid_adapter));
    expect<std::invalid_argument>([&] {
        kernel.run(kotavel::http::ServerSettings{"", 8080, 0});
    }, "empty bind hosts must fail before starting the server");
    expect<std::invalid_argument>([&] {
        kernel.run(kotavel::http::ServerSettings{"127.0.0.1", 0, 0});
    }, "port zero must fail before starting the server");

    expect<std::invalid_argument>([] {
        kotavel::http::HttpKernel invalid(nullptr);
    }, "a kernel without a transport adapter must fail at construction");
}

class ThrowingServer final : public kotavel::http::Server {
public:
    void run(const kotavel::http::HttpKernel&, const kotavel::http::ServerSettings&,
             const kotavel::http::ServerStartedCallback&) override {
        throw std::runtime_error("adapter failure");
    }
    void stop() noexcept override { stopped = true; }
    bool stopped = false;
};

// Fallos del adaptador se propagan y no permiten reabrir un kernel de un solo uso.
void test_http_kernel_adapter_failure() {
    auto adapter = std::make_unique<ThrowingServer>();
    auto* observed = adapter.get();
    kotavel::http::HttpKernel kernel(std::move(adapter));
    kernel.routes().api_get("/health", [](const auto&) { return kotavel::http::Response{}; });

    expect_error<std::runtime_error>([&] { kernel.run(); }, "adapter failure",
        "adapter exceptions should reach the application caller");
    check(kernel.routes().sealed(), "the route table should remain immutable after a start attempt");
    expect<std::logic_error>([&] { kernel.run(); }, "a failed server cannot reuse the single-use kernel");
    kernel.stop();
    check(observed->stopped, "explicit shutdown should be forwarded to the adapter after a failure");
}

}

int main() {
    try {
        test_controller_member_routes();
        test_http_route_groups();
        test_laravel_style_route_dsl();
        test_named_routes_and_url_generation();
        test_every_http_verb_and_request_view();
        test_http_kernel_middleware_pipeline();
        test_string_middleware_aliases();
        test_route_registration_validation_and_conflicts();
        test_nested_route_scopes_and_sealing();
        test_http_error_status_responses();
        test_http_kernel_defaults_and_validation();
        test_http_kernel_adapter_failure();
        std::cout << "HTTP routes and middleware: OK\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
