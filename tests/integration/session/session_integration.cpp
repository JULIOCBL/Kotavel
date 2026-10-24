#include <kotavel/http/server.hpp>
#include <kotavel/session/session.hpp>
#include <kotavel/http/session_middleware.hpp>

#include <chrono>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>

using namespace kotavel::http;

namespace {
class NoopServer final : public Server {
public:
    void run(const HttpKernel&, const ServerSettings&, const ServerStartedCallback&) override {}
    void stop() noexcept override {}
};

void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

std::string cookie_value(const Response& response) {
    const auto found = response.headers.find("Set-Cookie");
    require(found != response.headers.end(), "web response must set a session cookie");
    const auto begin = found->second.find('=') + 1;
    const auto end = found->second.find(';', begin);
    return found->second.substr(begin, end - begin);
}

Request request(Method method, RouteKind kind, std::string path, std::string cookie = {},
                std::string body = {}) {
    RequestData data;
    data.method = method;
    data.kind = kind;
    data.path = std::move(path);
    data.raw_method = method == Method::get ? "GET" : "POST";
    if (!cookie.empty()) data.headers.emplace("Cookie", std::move(cookie));
    if (!body.empty()) {
        data.headers.emplace("Content-Type", "application/x-www-form-urlencoded");
        data.body = std::move(body);
    }
    return Request(std::move(data));
}
}

int main() {
    const auto root = std::filesystem::temp_directory_path() /
        ("kotavel-session-integration-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));
    try {
        SessionConfig config;
        config.database = root / "sessions.sqlite3";
        config.cookie = "integration_session";
        auto context_store = std::make_shared<SQLiteSessionStore>(config.database);
        auto resolver = std::make_shared<SessionContextResolver>(context_store);
        const auto context = resolver->create_context();
        resolver->put(context, "company.id", JsonValue{15});
        const auto setup_kernel = [&](HttpKernel& kernel) {
            kernel.use_web(start_configured_session(config, root));
            kernel.routes().web_get("/counter", [](const Request& current) {
                const auto visits = current.session().get<int>("visits", 0) + 1;
                current.session().put("visits", visits);
                return Response{200, "text/plain; charset=utf-8", std::to_string(visits)};
            });
            kernel.routes().web_get("/csrf", [](const Request&) {
                return Response{200, "text/plain; charset=utf-8", Session::csrf_token()};
            });
            kernel.routes().web_post("/link", [resolver](const Request& current) {
                const auto submitted_csrf = current.input("_token");
                if (!submitted_csrf || !Session::verify_csrf_token(*submitted_csrf))
                    return Response{403, "text/plain", "CSRF token rejected"};
                const auto submitted = current.input("grant");
                if (!submitted || !resolver->redeem_link_grant(
                        SessionLinkGrant{std::string(*submitted)}, Session::id()))
                    return Response{403, "text/plain", "link rejected"};
                const auto company = resolver->get_for_session(Session::id(), "company.id");
                if (!company) return Response{403, "text/plain", "context unavailable"};
                return Response{200, "text/plain", std::to_string(
                    std::get<std::int64_t>(company->value()))};
            });
            kernel.routes().api_get("/health", [](const Request&) {
                return Response{200, "application/json", "{\"ok\":true}"};
            });
        };

        std::string id;
        {
            HttpKernel kernel(std::make_unique<NoopServer>());
            setup_kernel(kernel);
            const auto& routes = kernel.routes().routes();
            require(routes.size() == 4, "web and API routes should register");
            const auto first = kernel.handle(routes[0], request(Method::get, RouteKind::web, "/counter"));
            require(first.status == 200 && first.body == "1", "first web request starts the counter");
            id = cookie_value(first);
            require(id.size() == 64, "web cookie contains the opaque session identifier");
            require(std::filesystem::exists(config.database),
                    "configured SQLite store creates its database");
            const auto csrf = kernel.handle(routes[1], request(Method::get, RouteKind::web,
                "/csrf", "integration_session=" + id));
            require(csrf.status == 200 && csrf.body.size() == 64 && cookie_value(csrf) == id,
                    "Desktop-linked web session receives a persisted CSRF token");
            const auto second = kernel.handle(routes[0], request(Method::get, RouteKind::web,
                "/counter", "integration_session=" + id));
            require(second.status == 200 && second.body == "2" && cookie_value(second) == id,
                    "same client ID restores and updates the SQLite-backed session");
            const auto grant = resolver->issue_link_grant(context);
            const auto missing_csrf = kernel.handle(routes[2], request(Method::post, RouteKind::web,
                "/link", "integration_session=" + id, "grant=" + grant.value));
            require(missing_csrf.status == 403,
                    "Desktop link grant is not consumed by a request without a valid CSRF token");
            const auto linked = kernel.handle(routes[2], request(Method::post, RouteKind::web,
                "/link", "integration_session=" + id,
                "grant=" + grant.value + "&_token=" + csrf.body));
            require(linked.status == 200 && linked.body == "15",
                    "an explicitly authorized and CSRF-verified HTTP session redeems the Desktop context grant");
        }
        {
            HttpKernel restarted(std::make_unique<NoopServer>());
            setup_kernel(restarted);
            const auto& routes = restarted.routes().routes();
            const auto restored = restarted.handle(routes[0], request(Method::get, RouteKind::web,
                "/counter", "integration_session=" + id));
            require(restored.status == 200 && restored.body == "3" && cookie_value(restored) == id,
                    "SQLite session survives kernel and middleware reconstruction");
            const auto shared = resolver->get_for_session(id, "company.id");
            require(shared && std::get<std::int64_t>(shared->value()) == 15,
                    "the Desktop-to-Web context link persists across kernel reconstruction");
            const auto api = restarted.handle(routes[3], request(Method::get, RouteKind::api,
                "/api/health"));
            require(api.status == 200 && api.body == "{\"ok\":true}", "API route remains available");
            require(!api.headers.contains("Set-Cookie"), "API stays stateless without session middleware");
        }

        std::error_code ignored;
        std::filesystem::remove_all(root, ignored);
        std::cout << "HTTP session integration passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::error_code ignored;
        std::filesystem::remove_all(root, ignored);
        std::cerr << "FAIL: " << error.what() << '\n';
        return 1;
    }
}
