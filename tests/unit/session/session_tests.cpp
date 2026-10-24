#include <kotavel/env/env.hpp>
#include <kotavel/session/session.hpp>
#include <kotavel/http/session_middleware.hpp>
#include <kotavel/http/server.hpp>

#include <algorithm>
#include <array>
#include <barrier>
#include <chrono>
#include <atomic>
#include <condition_variable>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <latch>
#include <limits>
#include <map>
#include <mutex>
#include <stdexcept>
#include <string>
#include <thread>
#include <type_traits>
#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

using namespace kotavel;
using namespace kotavel::http;

namespace {
static_assert(std::is_same_v<
    decltype(&kotavel::http::detail::SessionScope::save),
    void (kotavel::http::detail::SessionScope::*)()>);

int failures = 0;

void expect(bool condition, const char* message) {
    if (!condition) {
        ++failures;
        std::cerr << "FAIL: " << message << '\n';
    }
}

class TestEnvironment final : public EnvironmentSource {
public:
    std::map<std::string, std::string, std::less<>> values;
    std::optional<std::string> read(std::string_view key) const override {
        const auto found = values.find(key);
        return found == values.end() ? std::nullopt : std::optional<std::string>(found->second);
    }
};

class NoopServer final : public Server {
public:
    void run(const HttpKernel&, const ServerSettings&, const ServerStartedCallback&) override {}
    void stop() noexcept override {}
};

class AlternateSessionTransport final : public contracts::SessionIdentifierTransport {
public:
    std::optional<std::string> read(const Request& request) const override {
        const auto value = request.header("X-Alternate-Session");
        if (!value) return std::nullopt;
        return std::string(*value);
    }
    void write(Response& response, std::string_view id) const override {
        response.headers.insert_or_assign("X-Alternate-Session", std::string(id));
    }
};

std::filesystem::path test_directory() {
    const auto timestamp = std::chrono::steady_clock::now().time_since_epoch().count();
#if defined(_WIN32)
    const auto process_id = _getpid();
#else
    const auto process_id = getpid();
#endif
    static std::atomic_uint64_t sequence{0};
    const auto parent = std::filesystem::temp_directory_path();
    for (int attempt = 0; attempt < 16; ++attempt) {
        const auto path = parent / ("kotavel-session-tests-" + std::to_string(process_id) + "-" +
            std::to_string(timestamp) + "-" +
            std::to_string(sequence.fetch_add(1, std::memory_order_relaxed)));
        if (std::filesystem::create_directory(path)) return path;
    }
    throw std::runtime_error("Could not allocate a unique session test directory");
}

void test_sqlite_store_initialization_failure_releases_handles(
    const std::filesystem::path& root) {
    const auto database = root / "failed-initialization" / "sessions.sqlite3";
    std::filesystem::create_directories(database.parent_path());
    {
        std::ofstream output(database, std::ios::binary);
        output << "not a SQLite database";
    }

    bool rejected = false;
    try {
        SQLiteSessionStore invalid(database, 2, 8);
    } catch (const std::exception&) {
        rejected = true;
    }
    expect(rejected, "SQLite initialization rejects a malformed database file");

    std::error_code remove_error;
    std::filesystem::remove(database, remove_error);
    expect(!remove_error, "failed SQLite initialization releases its database handles");
    SQLiteSessionStore recovered(database, 2, 8);
    expect(std::filesystem::exists(database),
           "a valid SQLite store can initialize after a failed constructor");
}

std::string cookie_id(const Response& response) {
    const auto found = response.headers.find("Set-Cookie");
    if (found == response.headers.end()) return {};
    const auto start = found->second.find('=');
    const auto end = found->second.find(';', start);
    return found->second.substr(start + 1, end - start - 1);
}

Request request_with_cookie(std::string cookie = {}) {
    RequestData data;
    data.method = Method::get;
    data.kind = RouteKind::web;
    data.path = "/session-test";
    data.raw_method = "GET";
    if (!cookie.empty()) data.headers.emplace("Cookie", std::move(cookie));
    return Request(std::move(data));
}

Request request_with_header(std::string name, std::string value = {}) {
    RequestData data;
    data.method = Method::get;
    data.kind = RouteKind::api;
    data.path = "/session-test";
    data.raw_method = "GET";
    if (!value.empty()) data.headers.emplace(std::move(name), std::move(value));
    return Request(std::move(data));
}

void test_round_trip_and_cookie_policy(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "sessions.sqlite3";
    config.lifetime = std::chrono::minutes(30);
    auto middleware = start_configured_session(config, root);

    auto first_request = request_with_cookie();
    const auto first_response = middleware(first_request, [&] {
        Session::put("user_id", 42);
        Session::put("preferences", JsonValue::Object{{"theme", "dark"}});
        session().put("welcome", true);
        expect(first_request.session().get<int>("user_id", 0) == 42,
               "Request::session shares the active session facade");
        return Response{200, "text/html", "saved"};
    });
    const auto id = cookie_id(first_response);
    expect(id.size() == 64, "session cookie contains a 256-bit hex identifier");
    expect(first_response.headers.contains("Set-Cookie"), "middleware sets a session cookie");
    const auto& cookie = first_response.headers.at("Set-Cookie");
    expect(cookie.find("Path=/") != std::string::npos, "cookie uses the configured browser path");
    expect(cookie.find("Max-Age=1800") != std::string::npos, "cookie lifetime follows session lifetime");
    expect(cookie.find("HttpOnly") != std::string::npos, "cookie is HttpOnly by default");
    expect(cookie.find("SameSite=Lax") != std::string::npos, "cookie uses SameSite=Lax by default");

    auto second_request = request_with_cookie("kotavel_session=" + id);
    const auto second_response = middleware(second_request, [&] {
        expect(Session::get<int>("user_id", 0) == 42, "scalar data persists across requests");
        expect(Session::get<bool>("welcome", false), "helper-style writes persist");
        const auto preferences = Session::get("preferences");
        const auto& object = std::get<JsonValue::Object>(preferences.value());
        expect(std::get<std::string>(object.at("theme").value()) == "dark",
               "nested JSON data persists across requests");
        expect(Session::has("user_id") && Session::exists("user_id"),
               "has and exists report stored values");
        const auto values = Session::all();
        expect(!values.contains("created_at") && !values.contains("expires_at"),
               "storage timestamps stay outside application session values");
        expect(Session::get<std::string>("absent", "fallback") == "fallback",
               "get returns its fallback for missing data");
        Session::put("nullable", nullptr);
        expect(Session::exists("nullable") && !Session::has("nullable"),
               "exists includes null while has excludes it");
        Session::forget("nullable");
        expect(!Session::exists("nullable"), "forget removes the requested key");
        return Response{200, "text/html", "loaded"};
    });
    expect(cookie_id(second_response) == id, "session identifier remains stable across requests");
}

void test_header_transport_isolation_and_restart(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "header-transport.sqlite3";
    config.transport = SessionTransport::header;
    config.header = "X-App-Session";
    config.cookie.clear();
    config.path.clear();
    config.same_site = SameSite::none;
    auto store = std::make_shared<SQLiteSessionStore>(config.database);
    auto middleware = start_session(config, store);

    const auto create_client = [&](std::string value) {
        auto request = request_with_header(config.header);
        const auto response = middleware(request, [&] {
            Session::put("client", value);
            return Response{200, "text/plain", value};
        });
        expect(!response.headers.contains("Set-Cookie"),
               "header transport does not emit a browser cookie");
        const auto found = response.headers.find(config.header);
        expect(found != response.headers.end(), "header transport returns the session identifier");
        return found == response.headers.end() ? std::string{} : found->second;
    };

    const auto client_a = create_client("A");
    const auto client_b = create_client("B");
    expect(client_a.size() == 64 && client_b.size() == 64 && client_a != client_b,
           "clients without identifiers receive distinct opaque IDs");

    const auto read_client = [&](const std::string& id) {
        auto request = request_with_header(config.header, id);
        std::string value;
        const auto response = middleware(request, [&] {
            value = Session::get<std::string>("client", "missing");
            return Response{200, "text/plain", value};
        });
        expect(response.headers.at(config.header) == id,
               "valid IDs are returned unchanged by the configured transport");
        return value;
    };
    expect(read_client(client_a) == "A", "client A retrieves its isolated session data");
    expect(read_client(client_b) == "B", "client B retrieves its isolated session data");

    constexpr std::string_view unknown_id =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    auto unknown_request = request_with_header(config.header, std::string(unknown_id));
    const auto unknown_response = middleware(unknown_request, [&] {
        expect(!Session::has("client"), "unknown valid-shaped ID cannot load arbitrary data");
        Session::put("client", "new");
        return Response{200, "text/plain", "new"};
    });
    expect(unknown_response.headers.at(config.header) != unknown_id,
           "unknown ID is replaced with a newly generated identifier");

    // A new middleware/store instance represents a server restart while the
    // backing files and the client-held identifier remain available.
    auto restarted = start_configured_session(config, root);
    auto restart_request = request_with_header(config.header, client_a);
    (void)restarted(restart_request, [&] {
        expect(Session::get<std::string>("client", "missing") == "A",
               "session survives middleware/server reconstruction");
        return Response{200, "text/plain", "A"};
    });
}

void test_custom_identifier_transport(const std::filesystem::path& root) {
    SessionConfig config;
    config.cookie.clear(); // A custom transport does not depend on cookie settings.
    config.path.clear();
    auto middleware = start_session(config,
        std::make_shared<SQLiteSessionStore>(root / "custom-transport.sqlite3"),
        std::make_shared<AlternateSessionTransport>());

    auto first = request_with_header("X-Alternate-Session");
    const auto created = middleware(first, [&] {
        Session::put("transport", "custom");
        return Response{200, "text/plain", "created"};
    });
    const auto id = created.headers.at("X-Alternate-Session");
    expect(id.size() == 64, "custom transport receives a generated opaque identifier");

    auto second = request_with_header("X-Alternate-Session", id);
    (void)middleware(second, [&] {
        expect(Session::get<std::string>("transport", "missing") == "custom",
               "custom transport restores the session through the public contract");
        return Response{200, "text/plain", "restored"};
    });
}

void test_expiration_and_rotation(const std::filesystem::path& root) {
    auto store = std::make_shared<SQLiteSessionStore>(root / "expiration.sqlite3");
    constexpr std::string_view expired_id =
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    {
        auto lock = store->lock(expired_id);
        const auto now = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::system_clock::now().time_since_epoch()).count();
        store->save({std::string(expired_id), now - 100, now - 90, now - 1,
                     {{"old", "value"}}});
    }

    SessionConfig config;
    config.database = root / "expiration.sqlite3";
    auto middleware = start_session(config, store);
    auto expired_request = request_with_cookie("kotavel_session=" + std::string(expired_id));
    const auto fresh_response = middleware(expired_request, [&] {
        expect(!Session::has("old"), "expired data is not exposed");
        Session::put("new", "value");
        return Response{200, "text/plain", "new"};
    });
    const auto fresh_id = cookie_id(fresh_response);
    expect(fresh_id != expired_id, "expired session receives a fresh identifier");
    expect(!store->load(expired_id).has_value(), "expired session record is deleted");

    constexpr std::string_view orphaned_id =
        "10112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    {
        auto lock = store->lock(orphaned_id);
        const auto now = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::system_clock::now().time_since_epoch()).count();
        store->save({std::string(orphaned_id), now - 100, now - 90, now - 1, {}});
    }
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    for (int i = 0; i < 65; ++i) store->collect_expired(now);
    expect(!store->load(orphaned_id).has_value(),
           "garbage collection removes expired sessions that clients no longer request");

    auto rotate_request = request_with_cookie("kotavel_session=" + fresh_id);
    const auto rotated_response = middleware(rotate_request, [&] {
        const auto before = Session::id();
        Session::regenerate();
        expect(Session::id() != before, "regenerate rotates the identifier");
        expect(Session::has("new"), "regenerate preserves current values");
        return Response{200, "text/plain", "rotated"};
    });
    const auto rotated_id = cookie_id(rotated_response);
    expect(rotated_id != fresh_id, "rotated identifier is sent to the browser");
    expect(!store->load(fresh_id).has_value(), "old identifier is removed after regeneration");

    auto invalidate_request = request_with_cookie("kotavel_session=" + rotated_id);
    const auto invalidated_response = middleware(invalidate_request, [&] {
        Session::invalidate();
        expect(!Session::has("new"), "invalidate clears application values");
        return Response{200, "text/plain", "invalidated"};
    });
    const auto invalidated_id = cookie_id(invalidated_response);
    expect(invalidated_id != rotated_id, "invalidate rotates the identifier");
    expect(!store->load(rotated_id).has_value(), "invalidate removes the previous record");
    expect(store->load(invalidated_id).has_value(), "invalidate persists a fresh empty session");

    auto after_rotation = request_with_cookie("kotavel_session=" + rotated_id);
    const auto replacement = middleware(after_rotation, [&] {
        expect(!Session::has("new"), "the rotated identifier does not restore retired data");
        return Response{200, "text/plain", "fresh"};
    });
    expect(cookie_id(replacement) != rotated_id, "a retired identifier cannot resume the old session");
}

void test_absolute_session_lifetime(const std::filesystem::path& root) {
    auto store = std::make_shared<SQLiteSessionStore>(root / "absolute-expiration.sqlite3");
    constexpr std::string_view aged_id =
        "20112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    {
        auto lock = store->lock(aged_id);
        store->save({std::string(aged_id), now - 120, now, now + 600,
                     {{"secret", "old-session"}}});
    }

    SessionConfig config;
    config.lifetime = std::chrono::minutes(10);
    config.max_lifetime = std::chrono::minutes(1);
    auto middleware = start_session(config, store);
    auto request = request_with_cookie("kotavel_session=" + std::string(aged_id));
    const auto response = middleware(request, [&] {
        expect(!Session::has("secret"),
               "absolute expiration rejects an active session older than its maximum age");
        Session::put("fresh", true);
        return Response{200, "text/plain", "fresh"};
    });

    const auto fresh_id = cookie_id(response);
    expect(fresh_id != aged_id, "absolute expiration issues a fresh session identifier");
    expect(!store->load(aged_id), "absolute expiration revokes the aged record");
    const auto fresh = store->load(fresh_id);
    expect(fresh && fresh->expires_at <= fresh->created_at + 60,
           "inactivity renewal cannot extend a session beyond its absolute lifetime");
}

void test_invalid_cookie_config_and_request_scope(const std::filesystem::path& root) {
    auto store = std::make_shared<SQLiteSessionStore>(root / "config.sqlite3");
    SessionConfig config;
    config.database = root / "config.sqlite3";
    auto middleware = start_session(config, store);
    auto invalid_request = request_with_cookie("kotavel_session=../../etc/passwd");
    const auto response = middleware(invalid_request, [&] {
        expect(!Session::has("x"), "invalid cookie cannot address an existing session");
        Session::put("x", 1);
        return Response{200, "text/plain", "ok"};
    });
    expect(cookie_id(response).size() == 64, "invalid identifier is replaced with a safe one");

    SessionConfig file_driver_config;
    file_driver_config.driver = "file";
    bool configured_file_driver_rejected = false;
    try { (void)start_configured_session(file_driver_config, root); }
    catch (const std::invalid_argument&) { configured_file_driver_rejected = true; }
    expect(configured_file_driver_rejected,
           "configured startup rejects unsupported session drivers");

    bool no_session_threw = false;
    try { (void)session(); }
    catch (const std::logic_error&) { no_session_threw = true; }
    expect(no_session_threw, "helper fails clearly outside the session middleware scope");

    bool same_site_rejected = false;
    try {
        config.same_site = SameSite::none;
        (void)start_session(config, store);
    } catch (const std::invalid_argument&) { same_site_rejected = true; }
    expect(same_site_rejected, "SameSite=None requires Secure cookies");

    bool encryption_rejected = false;
    try {
        config.same_site = SameSite::lax;
        config.encrypt = true;
        (void)start_session(config, store);
    } catch (const std::invalid_argument&) { encryption_rejected = true; }
    expect(encryption_rejected, "unsupported session encryption cannot be enabled silently");
}

void test_session_csrf_tokens_are_private_and_rotate(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "csrf.sqlite3";
    auto store = std::make_shared<SQLiteSessionStore>(config.database);
    auto middleware = start_session(config, store);
    std::string original_token;
    std::string rotated_token;
    std::string original_id;
    const auto response = middleware(request_with_cookie(), [&] {
        original_id = Session::id();
        original_token = Session::csrf_token();
        expect(original_token.size() == 64 && Session::verify_csrf_token(original_token),
               "CSRF token is an unguessable session-bound value and verifies successfully");
        expect(!Session::verify_csrf_token(original_token + "x") &&
               !Session::verify_csrf_token("0000000000000000000000000000000000000000000000000000000000000000"),
               "CSRF verifier rejects malformed and incorrect tokens");
        expect(!Session::has("_kotavel.csrf_token") &&
               !Session::exists("_kotavel.csrf_token") &&
               !Session::all().contains("_kotavel.csrf_token") &&
               Session::get<std::string>("_kotavel.csrf_token", "hidden") == "hidden",
               "CSRF token is not exposed through ordinary session value helpers");

        bool reserved_key_rejected = false;
        try { Session::put("_kotavel.csrf_token", "attacker-controlled"); }
        catch (const std::invalid_argument&) { reserved_key_rejected = true; }
        expect(reserved_key_rejected, "applications cannot overwrite the internal CSRF token");

        Session::regenerate();
        expect(!Session::verify_csrf_token(original_token),
               "session ID regeneration revokes the previous CSRF token");
        rotated_token = Session::csrf_token();
        expect(rotated_token != original_token && Session::verify_csrf_token(rotated_token),
               "a rotated session receives a fresh CSRF token");
        return Response{200, "text/plain", "csrf checked"};
    });
    expect(response.status == 200 && cookie_id(response) != original_id,
           "CSRF token rotation persists with the regenerated session ID");
}

void test_concurrent_requests_do_not_lose_updates(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "concurrent.sqlite3";
    auto middleware = start_configured_session(config, root);
    auto create_request = request_with_cookie();
    const auto created = middleware(create_request, [&] {
        Session::put("count", 0);
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    constexpr std::size_t worker_count = 32;
    std::barrier handlers_ready(static_cast<std::ptrdiff_t>(worker_count));
    std::atomic_size_t started = 0;
    std::array<int, worker_count> statuses{};
    std::vector<std::thread> workers;
    workers.reserve(worker_count);
    for (std::size_t index = 0; index < worker_count; ++index) {
        workers.emplace_back([&, index] {
            auto request = request_with_cookie("kotavel_session=" + id);
            const auto response = middleware(request, [&] {
                started.fetch_add(1, std::memory_order_relaxed);
                const auto count = Session::get<int>("count", 0);
                handlers_ready.arrive_and_wait();
                Session::put("count", count + 1);
                return Response{200, "text/plain", "updated"};
            });
            statuses[index] = response.status;
        });
    }
    for (auto& worker : workers) worker.join();
    expect(started == worker_count,
           "same-session requests execute concurrently outside the store lock");
    const auto successful = std::count(statuses.begin(), statuses.end(), 200);
    const auto conflicts = std::count(statuses.begin(), statuses.end(), 409);
    expect(successful == 1 && conflicts == static_cast<int>(worker_count - 1),
           "optimistic revisions accept one same-key writer and reject stale snapshots");

    auto verify_request = request_with_cookie("kotavel_session=" + id);
    (void)middleware(verify_request, [&] {
        expect(Session::get<int>("count", 0) == 1,
               "stale concurrent writes never overwrite the committed revision");
        return Response{200, "text/plain", "verified"};
    });
}

void test_stale_session_rotation_cannot_resurrect_identity(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "stale-rotation.sqlite3";
    auto middleware = start_configured_session(config, root);
    const auto created = middleware(request_with_cookie(), [] {
        Session::put("account", "active");
        return Response{200, "text/plain", "created"};
    });
    const auto original_id = cookie_id(created);

    std::barrier both_loaded(2);
    std::latch fresh_rotation_committed(1);
    int fresh_status = 0;
    int stale_status = 0;
    std::string fresh_id;
    std::thread fresh_request([&] {
        const auto response = middleware(
            request_with_cookie("kotavel_session=" + original_id), [&] {
                both_loaded.arrive_and_wait();
                Session::regenerate();
                return Response{200, "text/plain", "rotated"};
            });
        fresh_status = response.status;
        if (response.headers.contains("Set-Cookie")) fresh_id = cookie_id(response);
        fresh_rotation_committed.count_down();
    });
    std::thread stale_request([&] {
        const auto response = middleware(
            request_with_cookie("kotavel_session=" + original_id), [&] {
                both_loaded.arrive_and_wait();
                fresh_rotation_committed.wait();
                Session::put("stale", "must-not-resurrect");
                Session::regenerate();
                return Response{200, "text/plain", "stale rotation"};
            });
        stale_status = response.status;
    });
    fresh_request.join();
    stale_request.join();

    expect(fresh_status == 200 && fresh_id.size() == 64 && fresh_id != original_id,
           "the request with the current revision can rotate the session ID");
    expect(stale_status == 409,
           "a stale request cannot commit a second rotation after the old ID is retired");
    SQLiteSessionStore verifier(config.database, 2, 16);
    expect(!verifier.load(original_id), "the retired identity stays deleted");
    const auto fresh = verifier.load(fresh_id);
    expect(fresh && !fresh->values.contains("stale"),
           "a stale rotation cannot resurrect a retired identity or its pending data");
}

void test_concurrent_disjoint_keys_rebase_without_lost_updates(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "disjoint-writes.sqlite3";
    auto middleware = start_configured_session(config, root);
    const auto created = middleware(request_with_cookie(), [] {
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    constexpr std::size_t worker_count = 32;
    std::barrier handlers_ready(static_cast<std::ptrdiff_t>(worker_count));
    std::array<int, worker_count> statuses{};
    std::vector<std::thread> workers;
    workers.reserve(worker_count);
    for (std::size_t index = 0; index < worker_count; ++index) {
        workers.emplace_back([&, index] {
            auto request = request_with_cookie("kotavel_session=" + id);
            const auto response = middleware(request, [&] {
                handlers_ready.arrive_and_wait();
                Session::put("independent." + std::to_string(index), index);
                return Response{200, "text/plain", "updated"};
            });
            statuses[index] = response.status;
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::all_of(statuses.begin(), statuses.end(), [](int status) { return status == 200; }),
           "concurrent updates to independent keys rebase successfully");

    (void)middleware(request_with_cookie("kotavel_session=" + id), [&] {
        for (std::size_t index = 0; index < worker_count; ++index) {
            const auto key = "independent." + std::to_string(index);
            expect(Session::get<std::uint64_t>(key, std::numeric_limits<std::uint64_t>::max()) == index,
                   "rebasing preserves every independent key update");
        }
        return Response{200, "text/plain", "verified"};
    });
}

void test_concurrent_disjoint_keys_across_store_instances(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "multi-store-disjoint.sqlite3";
    auto first_sqlite = std::make_shared<SQLiteSessionStore>(config.database, 4, 64);
    auto second_sqlite = std::make_shared<SQLiteSessionStore>(config.database, 4, 64);
    auto first_cache = std::make_shared<CachedSessionStore>(first_sqlite);
    auto second_cache = std::make_shared<CachedSessionStore>(second_sqlite);
    auto first_middleware = start_session(config, first_cache);
    auto second_middleware = start_session(config, second_cache);

    const auto created = first_middleware(request_with_cookie(), [] {
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    std::barrier handlers_ready(2);
    std::array<int, 2> statuses{};
    std::array<std::thread, 2> workers{
        std::thread([&] {
            const auto response = first_middleware(
                request_with_cookie("kotavel_session=" + id), [&] {
                    handlers_ready.arrive_and_wait();
                    Session::put("instance.first", "committed");
                    return Response{200, "text/plain", "updated"};
                });
            statuses[0] = response.status;
        }),
        std::thread([&] {
            const auto response = second_middleware(
                request_with_cookie("kotavel_session=" + id), [&] {
                    handlers_ready.arrive_and_wait();
                    Session::put("instance.second", "rebased");
                    return Response{200, "text/plain", "updated"};
                });
            statuses[1] = response.status;
        })};
    for (auto& worker : workers) worker.join();
    expect(statuses[0] == 200 && statuses[1] == 200,
           "independent SQLite store instances merge disjoint concurrent writes");

    SQLiteSessionStore verifier(config.database, 2, 16);
    const auto durable = verifier.load(id);
    expect(durable && durable->values.contains("instance.first") &&
           durable->values.contains("instance.second"),
           "disjoint writes survive when competing writers use separate store instances");
}

void test_rebased_updates_enforce_combined_session_size(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "merged-size-limit.sqlite3";
    config.max_session_bytes = 1500;
    auto middleware = start_configured_session(config, root);
    const auto created = middleware(request_with_cookie(), [] {
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    std::barrier handlers_ready(2);
    std::array<int, 2> statuses{};
    std::array<bool, 2> size_rejections{};
    std::array<bool, 2> unexpected_errors{};
    std::array<std::thread, 2> workers;
    for (std::size_t index = 0; index < workers.size(); ++index) {
        workers[index] = std::thread([&, index] {
            try {
                const auto response = middleware(
                    request_with_cookie("kotavel_session=" + id), [&, index] {
                        handlers_ready.arrive_and_wait();
                        Session::put("key." + std::to_string(index), std::string(900, 'x'));
                        return Response{200, "text/plain", "updated"};
                    });
                statuses[index] = response.status;
            } catch (const std::length_error&) {
                size_rejections[index] = true;
            } catch (...) {
                unexpected_errors[index] = true;
            }
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::count(statuses.begin(), statuses.end(), 200) == 1 &&
           std::count(size_rejections.begin(), size_rejections.end(), true) == 1 &&
           std::none_of(unexpected_errors.begin(), unexpected_errors.end(), [](bool value) { return value; }),
           "concurrent disjoint-key merges reject a combined session that exceeds its size limit");

    (void)middleware(request_with_cookie("kotavel_session=" + id), [] {
        expect(Session::has("key.0") != Session::has("key.1"),
               "the rejected merge does not persist a partial oversized session");
        return Response{200, "text/plain", "verified"};
    });
}

void test_concurrent_atomic_counter_updates(const std::filesystem::path& root) {
    SessionConfig config;
    config.database = root / "atomic-counter.sqlite3";
    auto middleware = start_configured_session(config, root);
    const auto created = middleware(request_with_cookie(), [] {
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    constexpr std::size_t worker_count = 32;
    std::barrier handlers_ready(static_cast<std::ptrdiff_t>(worker_count));
    std::array<int, worker_count> statuses{};
    std::vector<std::thread> workers;
    workers.reserve(worker_count);
    for (std::size_t index = 0; index < worker_count; ++index) {
        workers.emplace_back([&, index] {
            auto request = request_with_cookie("kotavel_session=" + id);
            const auto response = middleware(request, [&] {
                handlers_ready.arrive_and_wait();
                (void)Session::increment("counter");
                return Response{200, "text/plain", "incremented"};
            });
            statuses[index] = response.status;
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::all_of(statuses.begin(), statuses.end(), [](int status) { return status == 200; }),
           "atomic increments rebase without same-key conflicts");
    (void)middleware(request_with_cookie("kotavel_session=" + id), [=] {
        expect(Session::get<std::uint64_t>("counter", 0) == worker_count,
               "every concurrent atomic increment is durable");
        return Response{200, "text/plain", "verified"};
    });
}

void test_many_independent_sessions_run_concurrently(const std::filesystem::path& root) {
    constexpr std::size_t client_count = 64;
    SessionConfig config;
    config.database = root / "independent-sessions.sqlite3";
    auto middleware = start_configured_session(config, root);
    std::vector<std::string> ids;
    ids.reserve(client_count);
    for (std::size_t index = 0; index < client_count; ++index) {
        auto response = middleware(request_with_cookie(), [index] {
            Session::put("client", static_cast<std::uint64_t>(index));
            return Response{200, "text/plain", "created"};
        });
        ids.push_back(cookie_id(response));
    }

    std::barrier handlers_ready(static_cast<std::ptrdiff_t>(client_count));
    std::atomic_size_t active = 0;
    std::atomic_size_t peak_active = 0;
    std::array<int, client_count> statuses{};
    std::vector<std::thread> workers;
    workers.reserve(client_count);
    for (std::size_t index = 0; index < client_count; ++index) {
        workers.emplace_back([&, index] {
            auto request = request_with_cookie("kotavel_session=" + ids[index]);
            const auto response = middleware(request, [&, index] {
                const auto current = active.fetch_add(1, std::memory_order_relaxed) + 1;
                auto observed = peak_active.load(std::memory_order_relaxed);
                while (observed < current && !peak_active.compare_exchange_weak(
                    observed, current, std::memory_order_relaxed)) {}
                handlers_ready.arrive_and_wait();
                Session::put("completed", static_cast<std::uint64_t>(index));
                active.fetch_sub(1, std::memory_order_relaxed);
                return Response{200, "text/plain", "updated"};
            });
            statuses[index] = response.status;
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::all_of(statuses.begin(), statuses.end(), [](int status) { return status == 200; }),
           "independent session writes all commit successfully under concurrent load");
    expect(peak_active >= client_count - 2,
           "independent sessions do not serialize behind a shared request lock");

    for (std::size_t index = 0; index < client_count; ++index) {
        auto request = request_with_cookie("kotavel_session=" + ids[index]);
        (void)middleware(request, [index] {
            expect(Session::get<std::uint64_t>("completed", 0) == index,
                   "each concurrent client retains its isolated session values");
            return Response{200, "text/plain", "verified"};
        });
    }
}

void test_sqlite_store_concurrent_commits(const std::filesystem::path& root) {
    const auto database = root / "sqlite-concurrency" / "sessions.sqlite3";
    constexpr std::size_t session_count = 96;
    std::vector<contracts::SessionRecord> records(session_count);
    for (std::size_t index = 0; index < session_count; ++index) {
        auto& record = records[index];
        record.id = std::string(62, '0');
        constexpr char hex[] = "0123456789abcdef";
        record.id += hex[(index >> 4) & 0xf];
        record.id += hex[index & 0xf];
        record.created_at = 1;
        record.last_activity = 1;
        record.expires_at = 100000;
        record.revision = 0;
        record.values.emplace("owner", static_cast<std::uint64_t>(index));
    }

    {
        SQLiteSessionStore store(database, 8, 32, 5000, std::chrono::milliseconds(5));
        std::barrier commit_gate(static_cast<std::ptrdiff_t>(session_count));
        std::array<bool, session_count> committed{};
        std::vector<std::thread> workers;
        workers.reserve(session_count);
        for (std::size_t index = 0; index < session_count; ++index) {
            workers.emplace_back([&, index] {
                commit_gate.arrive_and_wait();
                committed[index] = store.commit(records[index]);
                if (committed[index]) {
                    const auto durable = store.load(records[index].id);
                    committed[index] = durable && durable->revision == records[index].revision;
                }
            });
        }
        for (auto& worker : workers) worker.join();
        expect(std::all_of(committed.begin(), committed.end(), [](bool value) { return value; }),
               "bounded SQLite writer queue commits many independent sessions");
        const auto write_stats = store.statistics();
        expect(write_stats.batched_write_jobs == session_count &&
               write_stats.max_batch_size > 1,
               "concurrent independent session commits share a durable SQLite transaction batch");
        for (std::size_t index = 0; index < session_count; ++index) {
            const auto loaded = store.load(records[index].id);
            expect(loaded.has_value(), "SQLite concurrent commit remains readable");
            if (loaded) {
                const auto owner = loaded->values.find("owner");
                bool owner_matches = false;
                if (owner != loaded->values.end()) {
                    const auto& value = owner->second.value();
                    if (const auto* unsigned_value = std::get_if<std::uint64_t>(&value))
                        owner_matches = *unsigned_value == index;
                    else if (const auto* signed_value = std::get_if<std::int64_t>(&value))
                        owner_matches = *signed_value >= 0 &&
                            static_cast<std::uint64_t>(*signed_value) == index;
                }
                expect(owner_matches,
                       "SQLite concurrent commits preserve the correct isolated payload");
            }
        }

        auto first = store.load(records.front().id);
        auto stale = store.load(records.front().id);
        expect(first.has_value() && stale.has_value(), "SQLite returns snapshots for conflict checks");
        if (first && stale) {
            first->values.insert_or_assign("winner", true);
            stale->values.insert_or_assign("loser", true);
            contracts::SessionRecord independent;
            independent.id = std::string(62, 'f') + "00";
            independent.created_at = 1;
            independent.last_activity = 1;
            independent.expires_at = 100000;
            independent.revision = 0;
            independent.values.emplace("owner", std::uint64_t{999});

            std::barrier race_gate(3);
            bool first_committed = false;
            bool stale_committed = false;
            bool independent_committed = false;
            std::thread first_writer([&] {
                race_gate.arrive_and_wait();
                first_committed = store.commit(*first);
            });
            std::thread stale_writer([&] {
                race_gate.arrive_and_wait();
                stale_committed = store.commit(*stale);
            });
            std::thread independent_writer([&] {
                race_gate.arrive_and_wait();
                independent_committed = store.commit(independent);
            });
            first_writer.join();
            stale_writer.join();
            independent_writer.join();
            expect(first_committed != stale_committed,
                   "a batched stale CAS conflicts without rolling back the winning commit");
            expect(independent_committed && store.load(independent.id).has_value(),
                   "a stale CAS in a batch does not prevent an unrelated session commit");
            const auto winner = store.load(records.front().id);
            expect(winner && (winner->values.contains("winner") != winner->values.contains("loser")),
                   "a batch preserves exactly one same-revision session winner");
        }
    }

    SQLiteSessionStore restarted(database, 4, 16);
    const auto recovered = restarted.load(records.front().id);
    expect(recovered && (recovered->values.contains("winner") !=
           recovered->values.contains("loser")),
           "SQLite WAL data and the winning revision survive store reconstruction");
}

void test_sharded_cache_handles_concurrent_reads_and_ttl(const std::filesystem::path& root) {
    auto backing = std::make_shared<SQLiteSessionStore>(root / "cache-loads.sqlite3");
    contracts::SessionRecord record;
    record.id = std::string(64, 'a');
    record.created_at = 1;
    record.last_activity = 1;
    record.expires_at = 100000;
    record.values.emplace("theme", "dark");
    backing->save(record);

    constexpr std::size_t cold_reader_count = 24;
    CachedSessionStore cold_cache(backing, 64, 1024 * 1024, std::chrono::seconds(5), 16);
    std::barrier cold_load_gate(static_cast<std::ptrdiff_t>(cold_reader_count));
    std::array<bool, cold_reader_count> cold_load_valid{};
    std::vector<std::thread> cold_readers;
    cold_readers.reserve(cold_reader_count);
    for (std::size_t index = 0; index < cold_reader_count; ++index) {
        cold_readers.emplace_back([&, index] {
            cold_load_gate.arrive_and_wait();
            const auto loaded = cold_cache.load(record.id);
            if (!loaded) return;
            const auto theme = loaded->values.find("theme");
            if (theme == loaded->values.end()) return;
            const auto* value = std::get_if<std::string>(&theme->second.value());
            cold_load_valid[index] = value && *value == "dark";
        });
    }
    for (auto& reader : cold_readers) reader.join();
    expect(std::all_of(cold_load_valid.begin(), cold_load_valid.end(), [](bool value) { return value; }),
           "simultaneous cache misses return complete, consistent session snapshots");
    const auto cold_stats = cold_cache.statistics();
    expect(cold_stats.hits + cold_stats.misses == cold_reader_count &&
           cold_stats.resident_sessions == 1 && cold_stats.resident_bytes > 0,
           "cache statistics account for cold concurrent requests and bounded occupancy");

    CachedSessionStore cache(backing, 64, 1024 * 1024, std::chrono::seconds(5), 8);
    const auto initial = cache.load(record.id);
    expect(initial.has_value(), "cache miss loads a persisted session");
    const auto first_snapshot = cache.load_snapshot(record.id);
    const auto second_snapshot = cache.load_snapshot(record.id);
    expect(first_snapshot && first_snapshot == second_snapshot,
           "hot session reads reuse the same immutable cached record without copying its value map");
    const auto before_hot_reads = cache.statistics();
    constexpr std::size_t reader_count = 24;
    constexpr std::size_t reads_per_thread = 250;
    std::array<bool, reader_count> valid{};
    std::vector<std::thread> workers;
    workers.reserve(reader_count);
    for (std::size_t index = 0; index < reader_count; ++index) {
        workers.emplace_back([&, index] {
            for (std::size_t iteration = 0; iteration < reads_per_thread; ++iteration) {
                const auto loaded = cache.load(record.id);
                if (!loaded || loaded->values.find("theme") == loaded->values.end()) return;
            }
            valid[index] = true;
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::all_of(valid.begin(), valid.end(), [](bool value) { return value; }),
           "sharded cache serves concurrent readers without corrupting snapshots");
    const auto hot_stats = cache.statistics();
    expect(hot_stats.hits - before_hot_reads.hits == reader_count * reads_per_thread &&
           hot_stats.misses == before_hot_reads.misses,
           "cache hit and miss counters report request-level hit rate under concurrent reads");
    expect(hot_stats.max_lock_wait_ns <= hot_stats.lock_wait_ns &&
           (hot_stats.contended_lock_acquisitions != 0 || hot_stats.lock_wait_ns == 0),
           "cache lock contention statistics keep coherent wait totals and maxima");

    contracts::SessionRecord large_record;
    large_record.id = std::string(64, 'b');
    large_record.created_at = 1;
    large_record.last_activity = 1;
    large_record.expires_at = 100000;
    large_record.values.emplace("payload", std::string(2048, 'x'));
    backing->save(large_record);
    CachedSessionStore borrowing_cache(backing, 16, 4096, std::chrono::seconds(5), 4);
    expect(borrowing_cache.load(large_record.id).has_value(),
           "a large session loads when the aggregate cache has room");
    const auto borrowing_stats = borrowing_cache.statistics();
    expect(borrowing_stats.resident_sessions == 1 && borrowing_stats.resident_bytes > 1024 &&
           borrowing_stats.resident_bytes <= 4096,
           "a shard can borrow unused cache bytes without exceeding the global memory limit");

    contracts::SessionRecord second_large_record = large_record;
    constexpr char hex[] = "0123456789abcdef";
    const auto shard_of = [](std::string_view id) {
        return std::hash<std::string_view>{}(id) % 4;
    };
    for (std::size_t index = 0; index < 16; ++index) {
        second_large_record.id = std::string(63, 'c') + hex[index];
        if (shard_of(second_large_record.id) != shard_of(large_record.id)) break;
    }
    expect(shard_of(second_large_record.id) != shard_of(large_record.id),
           "cache byte-budget test selects two different shards");
    backing->save(second_large_record);
    CachedSessionStore concurrent_budget_cache(backing, 16, 3500,
                                                std::chrono::seconds(5), 4);
    std::barrier budget_gate(2);
    std::thread first_budget_loader([&] {
        budget_gate.arrive_and_wait();
        (void)concurrent_budget_cache.load(large_record.id);
    });
    std::thread second_budget_loader([&] {
        budget_gate.arrive_and_wait();
        (void)concurrent_budget_cache.load(second_large_record.id);
    });
    first_budget_loader.join();
    second_budget_loader.join();
    const auto concurrent_budget_stats = concurrent_budget_cache.statistics();
    expect(concurrent_budget_stats.resident_bytes <= 3500 &&
           concurrent_budget_stats.resident_sessions <= 1,
           "concurrent cache admissions keep the shared byte budget bounded across shards");

    CachedSessionStore short_cache(backing, 64, 1024 * 1024,
                                   std::chrono::milliseconds(20), 4);
    (void)short_cache.load(record.id);
    std::this_thread::sleep_for(std::chrono::milliseconds(30));
    (void)short_cache.load(record.id);
    expect(short_cache.statistics().misses == 2,
           "expired cache entries count as misses after TTL revalidation");

    contracts::SessionRecord tightly_bounded_record = record;
    constexpr char hex_digits[] = "0123456789abcdef";
    bool found_nonzero_default_shard = false;
    for (std::size_t index = 0; index < 256; ++index) {
        tightly_bounded_record.id = std::string(63, 'd') + hex_digits[index % 16];
        if (std::hash<std::string_view>{}(tightly_bounded_record.id) % 64 != 0) {
            found_nonzero_default_shard = true;
            break;
        }
    }
    expect(found_nonzero_default_shard,
           "single-session cache test selects an ID outside shard zero");
    backing->save(tightly_bounded_record);
    CachedSessionStore single_session_cache(backing, 1, 1024 * 1024,
                                            std::chrono::seconds(5), 64);
    expect(single_session_cache.load(tightly_bounded_record.id).has_value() &&
           single_session_cache.statistics().resident_sessions == 1,
           "a one-session cache remains usable when configured with more shards than capacity");
}

void test_session_context_grants_are_isolated_and_one_time(const std::filesystem::path& root) {
    const auto database = root / "contexts" / "sessions.sqlite3";
    auto store = std::make_shared<SQLiteSessionStore>(database, 8, 64);
    SessionContextResolver resolver(store);
#if !defined(_WIN32)
    const auto database_permissions = std::filesystem::status(database).permissions();
    expect((database_permissions & (std::filesystem::perms::group_all |
            std::filesystem::perms::others_all)) == std::filesystem::perms::none,
           "SQLite session and grant database is private to the current user");
#endif
    const auto instance_id = resolver.instance_id();
    const auto context = resolver.create_context();
    const auto make_id = [](std::size_t index) {
        constexpr char hex[] = "0123456789abcdef";
        std::string id(62, 'c');
        id.push_back(hex[(index >> 4) & 0xf]);
        id.push_back(hex[index & 0xf]);
        return id;
    };
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    constexpr std::size_t contender_count = 24;
    std::array<std::string, contender_count> session_ids{};
    for (std::size_t index = 0; index < contender_count; ++index) {
        session_ids[index] = make_id(index);
        store->save({session_ids[index], now, now, now + 3600, {}, 1});
    }

    resolver.put(context, "company.id", JsonValue{15});
    expect(!resolver.get_for_session(session_ids[0], "company.id"),
           "an HTTP session cannot see context data before an explicit grant is redeemed");
    const auto grant = resolver.issue_link_grant(context);
    std::barrier redeem_gate(static_cast<std::ptrdiff_t>(contender_count));
    std::array<bool, contender_count> redeemed{};
    std::vector<std::thread> workers;
    workers.reserve(contender_count);
    for (std::size_t index = 0; index < contender_count; ++index) {
        workers.emplace_back([&, index] {
            redeem_gate.arrive_and_wait();
            redeemed[index] = resolver.redeem_link_grant(grant, session_ids[index]);
        });
    }
    for (auto& worker : workers) worker.join();
    expect(std::count(redeemed.begin(), redeemed.end(), true) == 1,
           "concurrent redemption consumes a link grant exactly once");

    const auto winner = static_cast<std::size_t>(
        std::distance(redeemed.begin(), std::find(redeemed.begin(), redeemed.end(), true)));
    const auto shared_value = resolver.get_for_session(session_ids[winner], "company.id");
    expect(shared_value && std::get<std::int64_t>(shared_value->value()) == 15,
           "the authorized HTTP session resolves Desktop-owned context data");
    for (std::size_t index = 0; index < contender_count; ++index) {
        if (index == winner) continue;
        expect(!resolver.get_for_session(session_ids[index], "company.id"),
               "sessions without a redeemed grant remain isolated from the shared context");
    }

    const auto second_grant = resolver.issue_link_grant(context);
    expect(resolver.redeem_link_grant(second_grant, session_ids[winner]) == false,
           "an already-linked session cannot silently switch contexts");
    expect(resolver.revoke_context_link(session_ids[winner]),
           "Desktop or HTTP authorization flow can revoke a session context link");
    expect(!resolver.get_for_session(session_ids[winner], "company.id"),
           "revoked links stop exposing context values immediately");

    const auto logout_grant = resolver.issue_link_grant(context);
    expect(resolver.redeem_link_grant(logout_grant, session_ids[1]),
           "a fresh session can be linked after explicit authorization");
    auto expiring_session = store->load(session_ids[1]);
    expect(expiring_session.has_value(), "the authorized HTTP session remains durable");
    if (expiring_session) {
        expiring_session->expires_at = now - 1;
        store->save(*expiring_session);
    }
    const auto persisted_expiration = store->load(session_ids[1]);
    const auto current_time = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    expect(persisted_expiration && persisted_expiration->expires_at < current_time,
           "the test fixture persists an expired linked session");
    expect(!resolver.get_for_session(session_ids[1], "company.id"),
           "an expired HTTP session cannot read a context value through a live link");
    store->remove(session_ids[1]);
    expect(!store->load(session_ids[1]), "removing a linked session deletes its durable record");
    expect(!resolver.context_for_session(session_ids[1]),
           "deleting or invalidating an HTTP session removes its context link");
    expect(!resolver.get_for_session(session_ids[1], "company.id"),
           "an expired or removed HTTP session cannot read its former shared context");

    resolver.put(context, "temporary", JsonValue{"short-lived"}, std::chrono::seconds(1));
    std::this_thread::sleep_for(std::chrono::milliseconds(1100));
    expect(!resolver.get(context, "temporary"),
           "individual context values enforce their TTL during reads");

    auto restarted_store = std::make_shared<SQLiteSessionStore>(database, 2, 16);
    SessionContextResolver restarted(restarted_store);
    expect(restarted.instance_id().value == instance_id.value,
           "installation identity and contexts survive SQLite store reconstruction");
    expect(restarted.get(context, "company.id").has_value(),
           "context values persist independently from short-lived grant records");

    auto remote_store = std::make_shared<SQLiteSessionStore>(root / "remote" / "sessions.sqlite3");
    SessionContextResolver remote(remote_store);
    expect(remote.instance_id().value != instance_id.value &&
           !remote.get_for_session(session_ids[winner], "company.id"),
           "a separate installation gets an independent context namespace");

    bool unknown_context_rejected = false;
    try { (void)remote.issue_link_grant(context); }
    catch (const std::invalid_argument&) { unknown_context_rejected = true; }
    expect(unknown_context_rejected,
           "a context identifier from another installation cannot mint a local grant");

    const auto default_database = root / "default-context" / "sessions.sqlite3";
    auto default_store = std::make_shared<SQLiteSessionStore>(default_database);
    SessionContextResolver default_resolver(default_store);
    const auto default_context = default_resolver.default_context();
    expect(default_resolver.default_context().value == default_context.value,
           "an installation reuses its stable default context");
    auto restarted_default_store = std::make_shared<SQLiteSessionStore>(default_database);
    SessionContextResolver restarted_default(restarted_default_store);
    expect(restarted_default.default_context().value == default_context.value,
           "default context identity survives process reconstruction");
    expect(restarted_default.delete_context(default_context) &&
           restarted_default.default_context().value != default_context.value,
           "deleting the default context creates a fresh context on next startup");

    bool excessive_grant_lifetime_rejected = false;
    try { (void)resolver.issue_link_grant(context, std::chrono::minutes(11)); }
    catch (const std::invalid_argument&) { excessive_grant_lifetime_rejected = true; }
    expect(excessive_grant_lifetime_rejected,
           "link grants have a bounded maximum lifetime");

    const auto expiring_grant = resolver.issue_link_grant(context, std::chrono::seconds(1));
    std::this_thread::sleep_for(std::chrono::milliseconds(1100));
    expect(!resolver.redeem_link_grant(expiring_grant, session_ids[0]),
           "expired link grants cannot be redeemed");

    const auto retired_context = resolver.create_context();
    const auto retired_grant = resolver.issue_link_grant(retired_context);
    expect(resolver.delete_context(retired_context),
           "revoking a context deletes its values, links, and outstanding grants");
    expect(!resolver.redeem_link_grant(retired_grant, session_ids[1]),
           "deleting a context revokes every outstanding grant for it");
}

void test_kernel_request_integration(const std::filesystem::path& root) {
    auto store = std::make_shared<SQLiteSessionStore>(root / "kernel.sqlite3");
    SessionConfig config;
    HttpKernel kernel(std::make_unique<NoopServer>());
    kernel.use_web(start_session(config, store));
    bool persisted_on_followup = false;
    kernel.routes().web_get("/profile", [&persisted_on_followup](const Request& request) {
        if (request.session().has("profile_seen")) persisted_on_followup = true;
        request.session().put("profile_seen", true);
        return Response{200, "text/html", "profile"};
    });

    auto request = request_with_cookie();
    const auto response = kernel.handle(kernel.routes().routes().front(), request);
    expect(response.status == 200 && response.body == "profile",
           "HttpKernel dispatches a session-enabled web route");
    expect(response.headers.contains("Set-Cookie"),
           "HttpKernel returns session cookie headers from middleware");

    auto followup = request_with_cookie("kotavel_session=" + cookie_id(response));
    const auto followup_response = kernel.handle(kernel.routes().routes().front(), followup);
    expect(followup_response.status == 200,
           "a subsequent kernel request reuses the persisted session");
    expect(persisted_on_followup, "the controller sees data saved by the prior kernel request");
}

void test_environment_configuration() {
    TestEnvironment source;
    source.values = {{"SESSION_LIFETIME", "45"}, {"SESSION_COOKIE", "lab_session"},
                     {"SESSION_MAX_LIFETIME", "240"},
                     {"SESSION_CACHE_SHARDS", "32"},
                     {"SESSION_MAX_BYTES", "8192"},
                     {"SESSION_READ_CONNECTIONS", "8"},
                     {"SESSION_WRITE_QUEUE_CAPACITY", "256"},
                     {"SESSION_WRITE_BATCH_WINDOW_US", "250"},
                     {"SESSION_PATH", "/portal"}, {"SESSION_HTTP_ONLY", "false"},
                     {"SESSION_SAME_SITE", "strict"}, {"SESSION_TRANSPORT", "header"},
                     {"SESSION_HEADER", "X-Lab-Session"}};
    Env environment(source);
    EnvScope active_environment(environment);
    Config config_map;
    config_map.merge("session", SessionConfig::load());
    const auto config = SessionConfig::from_config(config_map);
    expect(config.lifetime == std::chrono::minutes(45), "lifetime loads from environment");
    expect(config.max_lifetime == std::chrono::minutes(240),
           "absolute maximum lifetime loads from environment");
    expect(config.cache_shards == 32, "cache partition count loads from environment");
    expect(config.max_session_bytes == 8192, "per-session memory limit loads from environment");
    expect(config.sqlite_read_connections == 8, "SQLite read pool size loads from environment");
    expect(config.sqlite_write_queue_capacity == 256,
           "SQLite writer queue capacity loads from environment");
    expect(config.sqlite_write_batch_window == std::chrono::microseconds(250),
           "SQLite write batch delay loads from environment");
    expect(config.cookie == "lab_session", "cookie name loads from environment");
    expect(config.path == "/portal", "SESSION_PATH configures the browser cookie path");
    expect(!config.http_only, "boolean cookie options load from environment");
    expect(config.same_site == SameSite::strict, "SameSite option loads from environment");
    expect(config.transport == SessionTransport::header,
           "identifier transport loads from environment");
    expect(config.header == "X-Lab-Session", "identifier header loads from environment");
    expect(config.driver == "sqlite", "SQLite WAL is the default session driver");

    source.values["SESSION_LIFETIME"] = "0";
    bool invalid_lifetime_rejected = false;
    try { (void)SessionConfig::load(); }
    catch (const std::invalid_argument&) { invalid_lifetime_rejected = true; }
    expect(invalid_lifetime_rejected, "nonpositive lifetime is rejected");
}

void test_session_size_limit(const std::filesystem::path& root) {
    SessionConfig config;
    config.max_session_bytes = 512;
    auto store = std::make_shared<SQLiteSessionStore>(root / "size-limit.sqlite3");
    auto middleware = start_session(config, store);
    const auto created = middleware(request_with_cookie(), [] {
        Session::put("value", "safe");
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    bool oversized_rejected = false;
    try {
        (void)middleware(request_with_cookie("kotavel_session=" + id), [] {
            Session::put("value", std::string(2048, 'x'));
            return Response{200, "text/plain", "too large"};
        });
    } catch (const std::length_error&) {
        oversized_rejected = true;
    }
    expect(oversized_rejected, "a session write beyond its configured memory budget is rejected");
    (void)middleware(request_with_cookie("kotavel_session=" + id), [] {
        expect(Session::get<std::string>("value", "") == "safe",
               "an oversized replacement leaves the prior value intact");
        return Response{200, "text/plain", "verified"};
    });
}

void test_maintenance_is_scoped_and_does_not_overlap() {
    class MaintenanceStore final : public contracts::SessionStore {
    public:
        std::atomic_int collections{0};
        std::atomic_int completed{0};
        std::atomic_int active{0};
        std::atomic_int max_active{0};
        std::atomic_int errors{0};
        std::mutex mutex;
        std::condition_variable changed;
        std::function<void()> on_collect;
        bool wait_for_collections(int count) {
            std::unique_lock lock(mutex);
            return changed.wait_for(lock, std::chrono::seconds(3), [&] {
                return collections.load() >= count;
            });
        }
        bool wait_for_completed(int count) {
            std::unique_lock lock(mutex);
            return changed.wait_for(lock, std::chrono::seconds(3), [&] {
                return completed.load() >= count;
            });
        }
        std::unique_ptr<Lock> lock(std::string_view) override { return std::make_unique<Lock>(); }
        std::optional<contracts::SessionRecord> load(std::string_view) override { return std::nullopt; }
        void save(const contracts::SessionRecord&) override {}
        void remove(std::string_view) override {}
        void collect_expired(std::int64_t) override {
            ++collections;
            changed.notify_all();
            const auto currently_active = ++active;
            auto previous_max = max_active.load();
            while (currently_active > previous_max &&
                   !max_active.compare_exchange_weak(previous_max, currently_active)) {}
            try {
                if (on_collect) on_collect();
            } catch (...) {
                ++errors;
                --active;
                ++completed;
                changed.notify_all();
                throw;
            }
            --active;
            ++completed;
            changed.notify_all();
        }
    };
    auto first_store = std::make_shared<MaintenanceStore>();
    auto second_store = std::make_shared<MaintenanceStore>();
    auto first = start_session({}, first_store);
    auto shared = first;
    auto second = start_session({}, second_store);
    const auto next = [] { return Response{200, "text/plain", "ok"}; };
    std::latch entered{1};
    std::latch release{1};
    first_store->on_collect = [&] { entered.count_down(); release.wait(); };
    (void)first(request_with_cookie(), next);
    entered.wait();
    try {
        (void)second(request_with_cookie(), next);
        for (int i = 0; i < 200; ++i) (void)shared(request_with_cookie(), next);
        expect(first_store->collections == 1,
               "concurrent requests skip redundant collection while a sweep is running");
        expect(first_store->max_active == 1,
               "a middleware never runs overlapping expiration sweeps");
        expect(second_store->wait_for_collections(1),
               "a different session middleware has its own maintenance cadence");
        expect(second_store->collections == 1,
               "a different session middleware has its own maintenance cadence");
    } catch (...) {
        release.count_down();
        throw;
    }
    release.count_down();
    expect(first_store->wait_for_completed(1), "the background expiration sweep completes");
    first_store->on_collect = {};
    for (int i = 0; i < 100; ++i) (void)first(request_with_cookie(), next);
    expect(first_store->wait_for_completed(2), "maintenance resumes after a previous sweep");
    expect(first_store->collections == 2, "maintenance resumes after the previous sweep completes");

    auto failing_store = std::make_shared<MaintenanceStore>();
    failing_store->on_collect = [] { throw std::runtime_error("collection failure"); };
    auto failing = start_session({}, failing_store);
    for (int i = 0; i < 101; ++i) (void)failing(request_with_cookie(), next);
    expect(failing_store->wait_for_completed(2),
           "background maintenance retries after a failed expiration sweep");
    expect(failing_store->errors == 2 && failing_store->collections == 2,
           "maintenance failures stay off the request path and do not stop later sweeps");
}
}

int main(int argc, char** argv) {
    if (argc == 3 && std::string_view(argv[1]) == "--durable-crash-write") {
        SQLiteSessionStore store(std::filesystem::path(argv[2]), 2, 8);
        contracts::SessionRecord record;
        record.id = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        record.created_at = 1;
        record.last_activity = 1;
        record.expires_at = std::numeric_limits<std::int64_t>::max();
        record.revision = 0;
        record.values.emplace("marker", "committed-before-process-exit");
        record.values.emplace("counter", std::uint64_t{123456});
        if (!store.commit(record)) std::_Exit(1);
        // Simulate process termination after durable COMMIT without running
        // SQLite/store destructors or orderly shutdown hooks.
        std::_Exit(0);
    }
    if (argc == 3 && std::string_view(argv[1]) == "--durable-crash-read") {
        SQLiteSessionStore store(std::filesystem::path(argv[2]), 2, 8);
        const auto record = store.load(
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789");
        if (!record || record->revision != 1) return 1;
        const auto marker = record->values.find("marker");
        const auto counter = record->values.find("counter");
        if (marker == record->values.end() || counter == record->values.end()) return 1;
        const auto* marker_value = std::get_if<std::string>(&marker->second.value());
        const auto* unsigned_counter = std::get_if<std::uint64_t>(&counter->second.value());
        const auto* signed_counter = std::get_if<std::int64_t>(&counter->second.value());
        return marker_value && *marker_value == "committed-before-process-exit" &&
            ((unsigned_counter && *unsigned_counter == 123456) ||
             (signed_counter && *signed_counter == 123456)) ? 0 : 1;
    }
    if (argc == 4 && std::string_view(argv[1]) == "--collect-expired") {
        try {
            SQLiteSessionStore store(std::filesystem::path(argv[2]), 2, 8);
            store.collect_expired(std::stoll(argv[3]));
            return 0;
        } catch (const std::exception& error) {
            std::cerr << "Expired-row collection failed: " << error.what() << '\n';
            return 1;
        }
    }
    if (argc == 4 && std::string_view(argv[1]) == "--issue-link-grant") {
        try {
            SQLiteSessionStore store(std::filesystem::path(argv[2]), 2, 8);
            (void)store.issue_link_grant(SessionContextId{argv[3]}, std::chrono::seconds(60));
            return 0;
        } catch (const std::exception& error) {
            std::cerr << "Link grant issuance failed: " << error.what() << '\n';
            return 1;
        }
    }
    const auto root = test_directory();
    try {
        test_sqlite_store_initialization_failure_releases_handles(root);
        test_round_trip_and_cookie_policy(root);
        test_header_transport_isolation_and_restart(root);
        test_custom_identifier_transport(root);
        test_expiration_and_rotation(root);
        test_absolute_session_lifetime(root);
        test_invalid_cookie_config_and_request_scope(root);
        test_session_csrf_tokens_are_private_and_rotate(root);
        test_many_independent_sessions_run_concurrently(root);
        test_concurrent_requests_do_not_lose_updates(root);
        test_stale_session_rotation_cannot_resurrect_identity(root);
        test_concurrent_disjoint_keys_rebase_without_lost_updates(root);
        test_concurrent_disjoint_keys_across_store_instances(root);
        test_rebased_updates_enforce_combined_session_size(root);
        test_concurrent_atomic_counter_updates(root);
        test_sqlite_store_concurrent_commits(root);
        test_sharded_cache_handles_concurrent_reads_and_ttl(root);
        test_session_context_grants_are_isolated_and_one_time(root);
        test_kernel_request_integration(root);
        test_environment_configuration();
        test_session_size_limit(root);
        test_maintenance_is_scoped_and_does_not_overlap();
    } catch (const std::exception& error) {
        ++failures;
        std::cerr << "UNCAUGHT: " << error.what() << '\n';
    }
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    if (failures != 0) return 1;
    std::cout << "Session tests passed\n";
    return 0;
}
