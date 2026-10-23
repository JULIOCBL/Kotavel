#include <kotavel/env/env.hpp>
#include <kotavel/filesystem/filesystem.hpp>
#include <kotavel/session/session.hpp>
#include <kotavel/http/session_middleware.hpp>
#include <kotavel/http/server.hpp>

#include <algorithm>
#include <chrono>
#include <atomic>
#include <filesystem>
#include <iostream>
#include <latch>
#include <fstream>
#include <map>
#include <stdexcept>
#include <string>
#include <thread>

using namespace kotavel;
using namespace kotavel::http;

namespace {
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

class TrackingFilesystem final : public contracts::Filesystem {
public:
    void create_directories(const std::filesystem::path& path, bool restrict_permissions) override {
        ++directory_creations;
        local_.create_directories(path, restrict_permissions);
    }
    std::optional<std::string> read_file(const std::filesystem::path& path,
                                         std::size_t maximum_bytes) const override {
        ++reads;
        return local_.read_file(path, maximum_bytes);
    }
    void write_file_atomically(const std::filesystem::path& path, std::string_view bytes,
                               bool restrict_permissions) override {
        ++writes;
        if (fail_writes) throw std::runtime_error("injected write failure");
        local_.write_file_atomically(path, bytes, restrict_permissions);
    }
    bool remove_file(const std::filesystem::path& path) override {
        ++removals;
        return local_.remove_file(path);
    }
    void remove_files(const std::vector<std::filesystem::path>& paths) override {
        ++bulk_removals;
        largest_batch = std::max(largest_batch, paths.size());
        local_.remove_files(paths);
    }
    std::vector<std::filesystem::path> list_files(
        const std::filesystem::path& path) const override {
        ++listings;
        return local_.list_files(path);
    }
    std::unique_ptr<contracts::FileLock> lock_file(
        const std::filesystem::path& path) override {
        ++locks;
        if (on_lock) on_lock();
        return local_.lock_file(path);
    }

    mutable int reads = 0;
    mutable int listings = 0;
    int directory_creations = 0;
    int writes = 0;
    bool fail_writes = false;
    int removals = 0;
    int bulk_removals = 0;
    std::size_t largest_batch = 0;
    int locks = 0;
    std::function<void()> on_lock;
private:
    filesystem::LocalFilesystem local_;
};

std::filesystem::path test_directory() {
    const auto path = std::filesystem::temp_directory_path() /
        ("kotavel-session-tests-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));
    std::filesystem::create_directories(path);
    return path;
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

void test_store_creates_missing_directories(const std::filesystem::path& root) {
    const auto sessions = root / "created" / "nested" / "sessions";
    expect(!std::filesystem::exists(sessions),
           "session storage path starts absent for the automatic-creation test");

    FileSessionStore store(sessions);

    expect(std::filesystem::is_directory(sessions),
           "file session store creates a missing nested storage directory");
    expect(std::filesystem::is_directory(sessions / ".locks"),
           "file session store creates its lock directory automatically");
}

void test_store_uses_injected_filesystem(const std::filesystem::path& root) {
    auto filesystem = std::make_shared<TrackingFilesystem>();
    const auto sessions = root / "injected-filesystem";
    constexpr std::string_view id =
        "30112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    FileSessionStore store(sessions, filesystem);
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();

    {
        auto lock = store.lock(id);
        store.save({std::string(id), now, now, now + 600, {{"value", "stored"}}});
        const auto loaded = store.load(id);
        expect(loaded.has_value(), "injected filesystem can persist and load a session");
        store.remove(id);
    }
    expect(filesystem->directory_creations >= 2,
           "session store delegates directory creation to its filesystem contract");
    expect(filesystem->locks == 1 && filesystem->writes == 1 && filesystem->reads >= 1 &&
           filesystem->removals == 2,
           "session store delegates locking and file operations to its filesystem contract");
}

void test_store_recovers_directories_removed_at_runtime(const std::filesystem::path& root) {
    const auto sessions = root / "removed-at-runtime" / "sessions";
    FileSessionStore store(sessions);
    constexpr std::string_view id =
        "20112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();

    std::filesystem::remove_all(sessions);
    {
        auto lock = store.lock(id);
        expect(std::filesystem::is_directory(sessions / ".locks"),
               "locking recreates a session directory deleted while the store is alive");
        store.save({std::string(id), now, now, now + 3600, {{"value", "restored"}}});
    }
    expect(store.load(id).has_value(),
           "save and load work after runtime recreation of the session directory");

    std::filesystem::remove_all(sessions / ".locks");
    {
        auto lock = store.lock(id);
        expect(std::filesystem::is_directory(sessions / ".locks"),
               "locking recreates a deleted lock subdirectory");
    }

    std::filesystem::remove_all(sessions);
    store.collect_expired(now);
    expect(std::filesystem::is_directory(sessions) &&
           std::filesystem::is_directory(sessions / ".locks"),
           "expiration collection recovers when the entire session directory is missing");
}

void test_expired_sessions_are_removed_in_shard_batches(const std::filesystem::path& root) {
    auto filesystem = std::make_shared<TrackingFilesystem>();
    FileSessionStore store(root / "bulk-expiration", filesystem);
    constexpr std::string_view first =
        "abc0000000000000000000000000000000000000000000000000000000000000";
    constexpr std::string_view second =
        "abc1111111111111111111111111111111111111111111111111111111111111";
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    for (const auto id : {first, second}) {
        auto lock = store.lock(id);
        store.save({std::string(id), now - 120, now - 120, now - 60, {}});
    }

    for (int i = 0; i < 65; ++i) store.collect_expired(now);

    expect(!store.load(first) && !store.load(second),
           "bulk collection removes every expired session in a lock shard");
    expect(filesystem->bulk_removals == 1 && filesystem->largest_batch == 2,
           "expired files sharing a lock shard are sent to the filesystem as one batch");
    expect(filesystem->locks == 3,
           "garbage collection acquires one shard lock for multiple session files");
}


void test_sharding_and_legacy_migration(const std::filesystem::path& root) {
    const auto directory = root / "migration";
    auto fs = std::make_shared<TrackingFilesystem>();
    FileSessionStore store(directory, fs);
    const std::string id = "ab" + std::string(62, '1');
    const auto current = directory / "ab" / (id + ".json");
    const auto legacy = directory / (id + ".json");
    auto lock = store.lock(id);
    store.save({id, 100, 200, 900, {{"visits", 7}}});
    expect(std::filesystem::exists(current) && !std::filesystem::exists(legacy),
           "new sessions use stable ID paths inside prefix directories");
    const auto reads = fs->reads;
    (void)store.load(id);
    expect(fs->reads == reads + 1 && fs->listings == 0,
           "normal lookup performs one read and never enumerates directories");
    std::filesystem::rename(current, legacy);
    fs->fail_writes = true;
    bool failed = false;
    try { (void)store.load(id); } catch (const std::runtime_error&) { failed = true; }
    expect(failed && std::filesystem::exists(legacy),
           "failed migration preserves the original session");
    fs->fail_writes = false;
    const auto migrated = store.load(id);
    expect(migrated && migrated->id == id && migrated->created_at == 100 &&
           migrated->last_activity == 200 && migrated->expires_at == 900 &&
           migrated->values.contains("visits"),
           "migration preserves identity, expiration and application data");
    expect(std::filesystem::exists(current) && !std::filesystem::exists(legacy),
           "migration commits new storage before removing legacy storage");
    FileSessionStore restarted(directory);
    expect(restarted.load(id).has_value(), "migrated session survives reconstruction");

    std::filesystem::copy_file(current, legacy);
    std::ofstream(current) << "corrupt";
    expect(!store.load(id) && !std::filesystem::exists(legacy),
           "corrupt authoritative copy cannot resurrect a legacy duplicate");
    store.save({id, 100, 200, 900, {}});
    std::filesystem::copy_file(current, legacy);
    store.remove(id);
    expect(!std::filesystem::exists(current) && !std::filesystem::exists(legacy),
           "invalidation removes both layouts");
}


void test_cleanup_rechecks_renewal_under_lock(const std::filesystem::path& root) {
    auto fs = std::make_shared<TrackingFilesystem>();
    FileSessionStore store(root / "renewal", fs);
    const auto id = std::string(64, '0');
    store.save({id, 100, 100, 200, {}});
    auto held = store.lock(id);
    std::latch attempted{1};
    fs->on_lock = [&] { attempted.count_down(); };
    std::exception_ptr failure;
    std::thread collector([&] {
        try { store.collect_expired(500); }
        catch (...) { failure = std::current_exception(); attempted.count_down(); }
    });
    attempted.wait();
    try { store.save({id, 100, 500, 1000, {{"renewed", true}}}); }
    catch (...) { held.reset(); collector.join(); throw; }
    held.reset();
    collector.join();
    if (failure) std::rethrow_exception(failure);
    const auto record = store.load(id);
    expect(record && record->expires_at == 1000 && record->values.contains("renewed"),
           "cleanup rechecks expiration after acquiring lock and preserves concurrent renewal");
}

void test_incremental_collection(const std::filesystem::path& root) {
    const auto directory = root / "incremental";
    FileSessionStore store(directory);
    const auto make_id = [](unsigned value) {
        auto id = std::string(64, '0');
        constexpr char hex[] = "0123456789abcdef";
        for (int i = 63; value; --i, value /= 16) id[i] = hex[value % 16];
        return id;
    };
    for (unsigned i = 1; i <= 140; ++i)
        store.save({make_id(i), 100, 100, 200, {}});
    const auto active = make_id(141);
    store.save({active, 100, 100, 1000, {}});
    const auto distant = "ff" + std::string(62, '1');
    store.save({distant, 100, 100, 200, {}});
    store.collect_expired(500);
    std::size_t remaining = 0;
    for (const auto& entry : std::filesystem::directory_iterator(directory / "00"))
        if (entry.is_regular_file()) ++remaining;
    expect(remaining >= 13 && remaining <= 14,
           "one maintenance call inspects at most 128 records");
    for (int i = 0; i < 66; ++i) store.collect_expired(500);
    for (unsigned i = 1; i <= 140; ++i)
        expect(!store.load(make_id(i)), "resumable cleanup eventually removes every expired record");
    expect(store.load(active).has_value(), "incremental cleanup preserves active records");
    expect(!store.load(distant), "cleanup reaches the last prefix directory");
}



void test_expiration_boundary_and_active_record(const std::filesystem::path& root) {
    FileSessionStore store(root / "expiration-boundary");
    const auto id_for = [](std::string_view prefix) {
        return std::string(prefix) + std::string(64 - prefix.size(), 'b');
    };
    const auto boundary = id_for("10");
    const auto active = id_for("ef");
    store.save({boundary, 100, 200, 500, {}});
    store.save({active, 100, 200, 501, {{"keep", true}}});

    store.collect_expired(500);

    expect(!store.load(boundary),
           "record expires at the exact cleanup timestamp");
    const auto surviving = store.load(active);
    expect(surviving && surviving->expires_at == 501 && surviving->values.contains("keep"),
           "cleanup preserves a record expiring after the sweep timestamp");
}

void test_sparse_expired_records_are_reached_in_one_sweep(const std::filesystem::path& root) {
    FileSessionStore store(root / "sparse-sweep");
    const auto now = std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    std::vector<std::string> ids;
    for (const auto prefix : {"04", "32", "77", "ca"}) {
        const auto id = std::string(prefix) + std::string(62, 'a');
        ids.push_back(id);
        store.save({id, now - 500, now - 400, now - 300, {}});
    }

    store.collect_expired(now);
    for (const auto& id : ids) {
        expect(!std::filesystem::exists(root / "sparse-sweep" / id.substr(0, 2) /
                                        (id + ".json")),
               "one sweep reaches expired sessions across sparse prefix directories");
    }
}

void test_round_trip_and_cookie_policy(const std::filesystem::path& root) {
    SessionConfig config;
    config.files = root / "sessions";
    config.lifetime = std::chrono::minutes(30);
    auto middleware = start_file_session(config, root);

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
    config.files = root / "header-transport";
    config.transport = SessionTransport::header;
    config.header = "X-App-Session";
    config.cookie.clear();
    config.path.clear();
    config.same_site = SameSite::none;
    auto store = std::make_shared<FileSessionStore>(config.files);
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
    auto restarted = start_file_session(config, root);
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
        std::make_shared<FileSessionStore>(root / "custom-transport"),
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
    auto store = std::make_shared<FileSessionStore>(root / "expiration");
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
    config.files = root / "expiration";
    auto middleware = start_session(config, store);
    auto expired_request = request_with_cookie("kotavel_session=" + std::string(expired_id));
    const auto fresh_response = middleware(expired_request, [&] {
        expect(!Session::has("old"), "expired data is not exposed");
        Session::put("new", "value");
        return Response{200, "text/plain", "new"};
    });
    const auto fresh_id = cookie_id(fresh_response);
    expect(fresh_id != expired_id, "expired session receives a fresh identifier");
    expect(!store->load(expired_id).has_value(), "expired session file is deleted");

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

void test_invalid_cookie_config_and_request_scope(const std::filesystem::path& root) {
    auto store = std::make_shared<FileSessionStore>(root / "config");
    SessionConfig config;
    config.files = root / "config";
    auto middleware = start_session(config, store);
    auto invalid_request = request_with_cookie("kotavel_session=../../etc/passwd");
    const auto response = middleware(invalid_request, [&] {
        expect(!Session::has("x"), "invalid cookie cannot address an existing session");
        Session::put("x", 1);
        return Response{200, "text/plain", "ok"};
    });
    expect(cookie_id(response).size() == 64, "invalid identifier is replaced with a safe one");

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

void test_concurrent_requests_do_not_lose_updates(const std::filesystem::path& root) {
    SessionConfig config;
    config.files = root / "concurrent";
    auto middleware = start_file_session(config, root);
    auto create_request = request_with_cookie();
    const auto created = middleware(create_request, [&] {
        Session::put("count", 0);
        return Response{200, "text/plain", "created"};
    });
    const auto id = cookie_id(created);
    std::atomic_int started = 0;
    const auto increment = [&] {
        auto request = request_with_cookie("kotavel_session=" + id);
        (void)middleware(request, [&] {
            ++started;
            std::this_thread::sleep_for(std::chrono::milliseconds(30));
            const auto count = Session::get<int>("count", 0);
            Session::put("count", count + 1);
            return Response{200, "text/plain", "updated"};
        });
    };
    std::thread first(increment);
    std::thread second(increment);
    first.join();
    second.join();
    expect(started == 2, "both concurrent session requests reach the handler");

    auto verify_request = request_with_cookie("kotavel_session=" + id);
    (void)middleware(verify_request, [&] {
        expect(Session::get<int>("count", 0) == 2,
               "file lock prevents concurrent requests from losing session updates");
        return Response{200, "text/plain", "verified"};
    });
}

void test_kernel_request_integration(const std::filesystem::path& root) {
    auto store = std::make_shared<FileSessionStore>(root / "kernel");
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
                     {"SESSION_PATH", "/portal"}, {"SESSION_HTTP_ONLY", "false"},
                     {"SESSION_SAME_SITE", "strict"}, {"SESSION_TRANSPORT", "header"},
                     {"SESSION_HEADER", "X-Lab-Session"}};
    Env environment(source);
    EnvScope active_environment(environment);
    Config config_map;
    config_map.merge("session", SessionConfig::load());
    const auto config = SessionConfig::from_config(config_map);
    expect(config.lifetime == std::chrono::minutes(45), "lifetime loads from environment");
    expect(config.cookie == "lab_session", "cookie name loads from environment");
    expect(config.path == "/portal", "SESSION_PATH configures the browser cookie path");
    expect(!config.http_only, "boolean cookie options load from environment");
    expect(config.same_site == SameSite::strict, "SameSite option loads from environment");
    expect(config.transport == SessionTransport::header,
           "identifier transport loads from environment");
    expect(config.header == "X-Lab-Session", "identifier header loads from environment");
    expect(config.driver == "file", "file driver is the default");

    source.values["SESSION_LIFETIME"] = "0";
    bool invalid_lifetime_rejected = false;
    try { (void)SessionConfig::load(); }
    catch (const std::invalid_argument&) { invalid_lifetime_rejected = true; }
    expect(invalid_lifetime_rejected, "nonpositive lifetime is rejected");
}

void test_maintenance_is_scoped_and_does_not_overlap() {
    class MaintenanceStore final : public contracts::SessionStore {
    public:
        std::atomic_int collections{0};
        std::function<void()> on_collect;
        std::unique_ptr<Lock> lock(std::string_view) override { return std::make_unique<Lock>(); }
        std::optional<contracts::SessionRecord> load(std::string_view) override { return std::nullopt; }
        void save(const contracts::SessionRecord&) override {}
        void remove(std::string_view) override {}
        void collect_expired(std::int64_t) override {
            ++collections;
            if (on_collect) on_collect();
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
    std::exception_ptr worker_error;
    std::thread worker([&] {
        try { (void)first(request_with_cookie(), next); }
        catch (...) { worker_error = std::current_exception(); }
    });
    entered.wait();
    try {
        (void)second(request_with_cookie(), next);
        for (int i = 0; i < 200; ++i) (void)shared(request_with_cookie(), next);
        expect(first_store->collections == 1,
               "concurrent requests skip redundant collection while a sweep is running");
        expect(second_store->collections == 1,
               "a different session middleware has its own maintenance cadence");
    } catch (...) {
        release.count_down();
        worker.join();
        throw;
    }
    release.count_down();
    worker.join();
    if (worker_error) std::rethrow_exception(worker_error);
    first_store->on_collect = {};
    for (int i = 0; i < 100; ++i) (void)first(request_with_cookie(), next);
    expect(first_store->collections == 2, "maintenance resumes after the previous sweep completes");

    auto failing_store = std::make_shared<MaintenanceStore>();
    failing_store->on_collect = [] { throw std::runtime_error("collection failure"); };
    auto failing = start_session({}, failing_store);
    int errors = 0;
    for (int i = 0; i < 101; ++i) {
        try { (void)failing(request_with_cookie(), next); }
        catch (const std::runtime_error&) { ++errors; }
    }
    expect(errors == 2 && failing_store->collections == 2,
           "failed collection releases its guard so later sweeps can retry");
}
}

int main() {
    const auto root = test_directory();
    try {
        test_store_creates_missing_directories(root);
        test_store_uses_injected_filesystem(root);
        test_store_recovers_directories_removed_at_runtime(root);
        test_expired_sessions_are_removed_in_shard_batches(root);
        test_sharding_and_legacy_migration(root);
        test_incremental_collection(root);
        test_sparse_expired_records_are_reached_in_one_sweep(root);
        test_expiration_boundary_and_active_record(root);
        test_cleanup_rechecks_renewal_under_lock(root);
        test_round_trip_and_cookie_policy(root);
        test_header_transport_isolation_and_restart(root);
        test_custom_identifier_transport(root);
        test_expiration_and_rotation(root);
        test_invalid_cookie_config_and_request_scope(root);
        test_concurrent_requests_do_not_lose_updates(root);
        test_kernel_request_integration(root);
        test_environment_configuration();
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
