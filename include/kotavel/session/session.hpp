#pragma once

#include <filesystem>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <memory>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>

#include <kotavel/contracts/session_store.hpp>
#include <kotavel/contracts/session_identifier_transport.hpp>
#include <kotavel/config/config.hpp>
#include <kotavel/http/api_response.hpp>

namespace kotavel::http {

enum class SameSite { lax, strict, none };
enum class SessionTransport { cookie, header };

struct SessionConfig final {
    std::string driver = "sqlite";
    // Relative paths are resolved against the consumer application's root.
    std::filesystem::path database = "storage/framework/sessions.sqlite3";
    std::chrono::minutes lifetime{120};
    // Absolute age limit, independent of inactivity renewal. Zero disables it.
    std::chrono::minutes max_lifetime{24 * 60};
    std::size_t cache_max_sessions = 2048;
    std::size_t cache_max_bytes = 32 * 1024 * 1024;
    std::size_t cache_shards = 64;
    std::chrono::milliseconds cache_ttl{1000};
    std::size_t max_session_bytes = 1024 * 1024;
    std::size_t sqlite_read_connections = 4;
    std::size_t sqlite_write_queue_capacity = 1024;
    std::chrono::microseconds sqlite_write_batch_window{0};
    std::string cookie = "kotavel_session";
    SessionTransport transport = SessionTransport::cookie;
    std::string header = "X-Kotavel-Session";
    std::string path = "/"; // Browser cookie path, not the directory above.
    std::optional<std::string> domain;
    bool secure = false;
    bool http_only = true;
    bool expire_on_close = false;
    bool encrypt = false;
    SameSite same_site = SameSite::lax;

    // Register load() as the "session" ConfigFile group, then resolve the
    // resulting immutable Config once during application bootstrap.
    [[nodiscard]] static kotavel::ConfigMap load();
    [[nodiscard]] static SessionConfig from_config(const kotavel::Config& configuration);
};

// Laravel-style facade scoped to the request currently executing on this
// worker thread. Use session() for the same API in instance-style expressions.
class Session final {
public:
    Session(const Session&) = delete;
    Session& operator=(const Session&) = delete;

    template <typename T>
    static void put(std::string_view key, const T& value) {
        put_json(key, detail::to_json(value));
    }
    static void put(std::string_view key, JsonValue value) { put_json(key, std::move(value)); }

    [[nodiscard]] static JsonValue get(std::string_view key,
                                       JsonValue fallback = JsonValue{});
    template <typename T>
    [[nodiscard]] static T get(std::string_view key, T fallback) {
        const auto value = get(key, JsonValue{});
        const auto& storage = value.value();
        if constexpr (std::is_same_v<T, JsonValue>) return value;
        else if constexpr (std::is_same_v<T, std::string>) {
            if (const auto* result = std::get_if<std::string>(&storage)) return *result;
        } else if constexpr (std::is_same_v<T, bool>) {
            if (const auto* result = std::get_if<bool>(&storage)) return *result;
        } else if constexpr (std::is_integral_v<T> && std::is_signed_v<T>) {
            if (const auto* result = std::get_if<std::int64_t>(&storage))
                if (*result >= static_cast<std::int64_t>(std::numeric_limits<T>::min()) &&
                    *result <= static_cast<std::int64_t>(std::numeric_limits<T>::max()))
                    return static_cast<T>(*result);
            if (const auto* result = std::get_if<std::uint64_t>(&storage))
                if (*result <= static_cast<std::uint64_t>(std::numeric_limits<T>::max()))
                    return static_cast<T>(*result);
        } else if constexpr (std::is_integral_v<T> && std::is_unsigned_v<T>) {
            if (const auto* result = std::get_if<std::uint64_t>(&storage))
                if (*result <= static_cast<std::uint64_t>(std::numeric_limits<T>::max()))
                    return static_cast<T>(*result);
            if (const auto* result = std::get_if<std::int64_t>(&storage))
                if (*result >= 0 && static_cast<std::uint64_t>(*result) <=
                    static_cast<std::uint64_t>(std::numeric_limits<T>::max()))
                    return static_cast<T>(*result);
        } else if constexpr (std::is_floating_point_v<T>) {
            if (const auto* result = std::get_if<double>(&storage)) return static_cast<T>(*result);
            if (const auto* result = std::get_if<std::int64_t>(&storage)) return static_cast<T>(*result);
            if (const auto* result = std::get_if<std::uint64_t>(&storage)) return static_cast<T>(*result);
        } else {
            static_assert(std::is_same_v<T, void>,
                "Session::get supports JsonValue, strings, booleans and numeric types");
        }
        return fallback;
    }
    [[nodiscard]] static bool has(std::string_view key);
    [[nodiscard]] static bool exists(std::string_view key);
    // Session-bound CSRF token for state-changing browser requests.
    [[nodiscard]] static std::string csrf_token();
    [[nodiscard]] static bool verify_csrf_token(std::string_view candidate);
    // Atomic nonnegative counter update. Concurrent increments rebase over the
    // latest durable revision; generic put/get changes remain conflict checked.
    static void increment(std::string_view key, std::uint64_t amount = 1);
    static void forget(std::string_view key);
    static void flush();
    [[nodiscard]] static JsonValue::Object all();
    [[nodiscard]] static std::string id();
    static void regenerate();
    static void invalidate();

private:
    Session() = default;
    static void put_json(std::string_view key, JsonValue value);
    friend Session& session();
};

// Session::put(...) and session().put(...) address the same active request.
[[nodiscard]] Session& session();

// Context identifiers and grants have distinct types from Session::id(). A
// grant is a short-lived bearer credential and should only be shown once.
struct SessionInstanceId final { std::string value; };
struct SessionContextId final { std::string value; };
struct SessionLinkGrant final { std::string value; };

class SQLiteSessionStore final : public contracts::SessionStore,
                                 public contracts::SessionRevisionStore,
                                 public contracts::SessionRotationStore {
public:
    struct Statistics final {
        std::uint64_t write_jobs = 0;
        std::uint64_t admission_wait_ns = 0;
        std::uint64_t queue_wait_ns = 0;
        std::uint64_t max_queue_wait_ns = 0;
        std::uint64_t service_ns = 0;
        std::uint64_t max_service_ns = 0;
        std::size_t peak_queue_depth = 0;
        std::uint64_t write_batches = 0;
        std::uint64_t batched_write_jobs = 0;
        std::size_t max_batch_size = 0;
    };

    explicit SQLiteSessionStore(std::filesystem::path database,
                                std::size_t read_connections = 4,
                                std::size_t max_queued_writes = 1024,
                                int busy_timeout_ms = 5000,
                                std::chrono::microseconds write_batch_window =
                                    std::chrono::microseconds{0});
    ~SQLiteSessionStore() override;
    SQLiteSessionStore(const SQLiteSessionStore&) = delete;
    SQLiteSessionStore& operator=(const SQLiteSessionStore&) = delete;

    [[nodiscard]] std::unique_ptr<contracts::SessionStore::Lock>
    lock(std::string_view id) override;
    [[nodiscard]] std::optional<contracts::SessionRecord>
    load(std::string_view id) override;
    void save(const contracts::SessionRecord& record) override;
    void remove(std::string_view id) override;
    bool commit(contracts::SessionRecord& record) override;
    bool commit_rotation(contracts::SessionRecord& replacement,
                        std::uint64_t expected_previous_revision,
                        std::span<const std::string> retired_ids) override;
    bool commit_rebased(contracts::SessionRecord& record,
                        const std::function<bool(contracts::SessionRecord&)>& rebase) override;
    void collect_expired(std::int64_t now) override;
    [[nodiscard]] Statistics statistics() const;

    [[nodiscard]] SessionInstanceId instance_id() const;
    [[nodiscard]] SessionContextId create_context();
    [[nodiscard]] SessionContextId default_context();
    [[nodiscard]] bool delete_context(const SessionContextId& context);
    [[nodiscard]] SessionLinkGrant issue_link_grant(
        const SessionContextId& context, std::chrono::seconds lifetime = std::chrono::seconds(60));
    [[nodiscard]] bool redeem_link_grant(const SessionLinkGrant& grant,
                                         std::string_view session_id);
    [[nodiscard]] std::optional<SessionContextId> context_for_session(
        std::string_view session_id);
    [[nodiscard]] bool revoke_context_link(std::string_view session_id);
    void put_context_value(const SessionContextId& context, std::string_view key,
                           JsonValue value,
                           std::optional<std::chrono::seconds> lifetime = std::nullopt);
    [[nodiscard]] std::optional<JsonValue> get_context_value(
        const SessionContextId& context, std::string_view key);
    [[nodiscard]] std::optional<JsonValue> get_context_value_for_session(
        std::string_view session_id, std::string_view key);

private:
    struct State;
    std::unique_ptr<State> state_;
};

// Explicit bridge between local Desktop state and an independently
// authenticated HTTP session. The grant issuer belongs in trusted local
// application code; HTTP handlers should only redeem a user-authorized grant.
class SessionContextResolver final {
public:
    explicit SessionContextResolver(std::shared_ptr<SQLiteSessionStore> store);

    [[nodiscard]] SessionInstanceId instance_id() const;
    [[nodiscard]] SessionContextId create_context();
    [[nodiscard]] SessionContextId default_context();
    [[nodiscard]] bool delete_context(const SessionContextId& context);
    [[nodiscard]] SessionLinkGrant issue_link_grant(
        const SessionContextId& context, std::chrono::seconds lifetime = std::chrono::seconds(60));
    [[nodiscard]] bool redeem_link_grant(const SessionLinkGrant& grant,
                                         std::string_view session_id);
    [[nodiscard]] std::optional<SessionContextId> context_for_session(
        std::string_view session_id);
    [[nodiscard]] bool revoke_context_link(std::string_view session_id);
    void put(const SessionContextId& context, std::string_view key, JsonValue value,
             std::optional<std::chrono::seconds> lifetime = std::nullopt);
    [[nodiscard]] std::optional<JsonValue> get(const SessionContextId& context,
                                                std::string_view key);
    [[nodiscard]] std::optional<JsonValue> get_for_session(std::string_view session_id,
                                                            std::string_view key);

private:
    std::shared_ptr<SQLiteSessionStore> store_;
};

// Bounded process-local read-through cache. The TTL limits staleness when more
// than one process uses the same durable store; revisions remain authoritative.
class CachedSessionStore final : public contracts::SessionStore,
                                 public contracts::SessionRevisionStore,
                                 public contracts::SessionSnapshotStore,
                                 public contracts::SessionRotationStore {
public:
    struct Statistics final {
        std::uint64_t hits = 0;
        std::uint64_t misses = 0;
        std::uint64_t evictions = 0;
        std::uint64_t contended_lock_acquisitions = 0;
        std::uint64_t lock_wait_ns = 0;
        std::uint64_t max_lock_wait_ns = 0;
        std::size_t resident_sessions = 0;
        std::size_t resident_bytes = 0;
    };

    explicit CachedSessionStore(std::shared_ptr<contracts::SessionStore> backing,
                                 std::size_t max_sessions = 2048,
                                 std::size_t max_bytes = 32 * 1024 * 1024,
                                 std::chrono::milliseconds ttl = std::chrono::milliseconds{1000},
                                 std::size_t shard_count = 64);
    ~CachedSessionStore() override;
    CachedSessionStore(const CachedSessionStore&) = delete;
    CachedSessionStore& operator=(const CachedSessionStore&) = delete;

    [[nodiscard]] std::unique_ptr<contracts::SessionStore::Lock>
    lock(std::string_view id) override;
    [[nodiscard]] std::optional<contracts::SessionRecord>
    load(std::string_view id) override;
    [[nodiscard]] std::shared_ptr<const contracts::SessionRecord>
    load_snapshot(std::string_view id) override;
    void save(const contracts::SessionRecord& record) override;
    void remove(std::string_view id) override;
    bool commit(contracts::SessionRecord& record) override;
    bool commit_rotation(contracts::SessionRecord& replacement,
                        std::uint64_t expected_previous_revision,
                        std::span<const std::string> retired_ids) override;
    bool commit_rebased(contracts::SessionRecord& record,
                        const std::function<bool(contracts::SessionRecord&)>& rebase) override;
    void collect_expired(std::int64_t now) override;
    [[nodiscard]] Statistics statistics() const;

private:
    struct State;
    std::unique_ptr<State> state_;
};

namespace detail {
class SessionScope final {
public:
    struct State;
    SessionScope(contracts::SessionStore& store, contracts::SessionRecord record,
                 const SessionConfig& config);
    SessionScope(contracts::SessionStore& store,
                 std::shared_ptr<const contracts::SessionRecord> record,
                 const SessionConfig& config);
    ~SessionScope();
    SessionScope(const SessionScope&) = delete;
    SessionScope& operator=(const SessionScope&) = delete;

    [[nodiscard]] contracts::SessionRecord& record() noexcept;
    void save();
    [[nodiscard]] bool try_save();

private:
    std::unique_ptr<State> state_;
};
} // namespace detail

} // namespace kotavel::http
