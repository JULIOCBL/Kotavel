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

Request request(Method method, RouteKind kind, std::string path,
                std::string cookie = {}) {
    RequestData data;
    data.method = method;
    data.kind = kind;
    data.path = std::move(path);
    data.raw_method = method == Method::get ? "GET" : "POST";
    if (!cookie.empty()) data.headers.emplace("Cookie", std::move(cookie));
    return Request(std::move(data));
}
}

int main() {
    const auto root = std::filesystem::temp_directory_path() /
        ("kotavel-session-integration-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));
    try {
        SessionConfig config;
        config.files = root / "sessions";
        config.cookie = "integration_session";

        const auto now = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::system_clock::now().time_since_epoch()).count();
        const auto id_for = [](std::string_view prefix) {
            return std::string(prefix) + std::string(64 - prefix.size(), 'a');
        };
        const auto expired_one = id_for("04");
        const auto expired_two = id_for("ca");
        const auto expired_legacy = id_for("7f");
        const auto active_id = id_for("32");
        {
            FileSessionStore seeded(config.files);
            seeded.save({expired_one, now - 600, now - 300, now - 1, {}});
            seeded.save({expired_two, now - 600, now - 300, now - 1, {}});
            seeded.save({expired_legacy, now - 600, now - 300, now - 1, {}});
            seeded.save({active_id, now, now, now + 3600, {{"marker", "preserve"}}});
            const auto legacy_shard = config.files / "7f" / (expired_legacy + ".json");
            std::filesystem::rename(legacy_shard, config.files / (expired_legacy + ".json"));
        }

        HttpKernel kernel(std::make_unique<NoopServer>());
        kernel.use_web(start_file_session(config, root));

        kernel.routes().web_get("/counter", [](const Request& current) {
            const auto visits = current.session().get<int>("visits", 0) + 1;
            current.session().put("visits", visits);
            return Response{200, "text/plain; charset=utf-8", std::to_string(visits)};
        });
        kernel.routes().api_get("/health", [](const Request&) {
            return Response{200, "application/json", "{\"ok\":true}"};
        });

        const auto& routes = kernel.routes().routes();
        require(routes.size() == 2, "both web and API routes should register");

        const auto first = kernel.handle(routes[0], request(Method::get, RouteKind::web, "/counter"));
        require(first.status == 200 && first.body == "1", "first web request starts the counter");
        const auto id = cookie_value(first);
        require(id.size() == 64, "web cookie contains the opaque session identifier");
        require(std::filesystem::exists(config.files / id.substr(0, 2) / (id + ".json")),
                "file store persists the session under the configured project directory");
        require(!std::filesystem::exists(config.files / expired_one.substr(0, 2) /
                                         (expired_one + ".json")) &&
                !std::filesystem::exists(config.files / expired_two.substr(0, 2) /
                                         (expired_two + ".json")),
                "first HTTP request removes expired sessions from separate prefix directories");
        require(!std::filesystem::exists(config.files / (expired_legacy + ".json")),
                "first HTTP request also removes expired legacy root files");
        FileSessionStore verify_store(config.files);
        const auto preserved = verify_store.load(active_id);
        require(preserved && preserved->values.contains("marker"),
                "HTTP cleanup preserves an active session in another prefix");

        const auto second = kernel.handle(routes[0], request(
            Method::get, RouteKind::web, "/counter", "integration_session=" + id));
        require(second.status == 200 && second.body == "2",
                "a later web request restores and updates the same session");
        require(cookie_value(second) == id, "session identifier stays stable between requests");

        const auto sharded = config.files / id.substr(0, 2) / (id + ".json");
        const auto legacy = config.files / (id + ".json");
        std::filesystem::rename(sharded, legacy);
        const auto migrated = kernel.handle(routes[0], request(
            Method::get, RouteKind::web, "/counter", "integration_session=" + id));
        require(migrated.status == 200 && migrated.body == "3" && cookie_value(migrated) == id,
                "legacy session migration preserves the browser cookie and counter");
        require(std::filesystem::exists(sharded) && !std::filesystem::exists(legacy),
                "HTTP request migrates legacy storage automatically");

        std::filesystem::remove_all(config.files);
        const auto after_storage_removal = kernel.handle(routes[0], request(
            Method::get, RouteKind::web, "/counter", "integration_session=" + id));
        require(after_storage_removal.status == 200 && after_storage_removal.body == "1",
                "removing the session directory at runtime starts a fresh session instead of returning 500");
        const auto recovered_id = cookie_value(after_storage_removal);
        require(recovered_id != id, "lost session data receives a fresh client identifier");
        require(std::filesystem::is_directory(config.files) &&
                std::filesystem::is_directory(config.files / ".locks"),
                "HTTP middleware recreates session and lock directories automatically");

        const auto after_recovery = kernel.handle(routes[0], request(
            Method::get, RouteKind::web, "/counter", "integration_session=" + recovered_id));
        require(after_recovery.status == 200 && after_recovery.body == "2",
                "subsequent requests persist normally after directory recovery");
        require(cookie_value(after_recovery) == recovered_id,
                "recovered session identifier remains stable on the next request");

        const auto api = kernel.handle(routes[1], request(Method::get, RouteKind::api, "/api/health"));
        require(api.status == 200 && api.body == "{\"ok\":true}", "API route remains available");
        require(!api.headers.contains("Set-Cookie"), "API stays stateless without session middleware");

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
