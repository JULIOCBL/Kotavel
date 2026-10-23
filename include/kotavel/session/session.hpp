#pragma once

#include <filesystem>
#include <chrono>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>

#include <kotavel/contracts/session_store.hpp>
#include <kotavel/contracts/session_identifier_transport.hpp>
#include <kotavel/config/config.hpp>
#include <kotavel/contracts/filesystem.hpp>
#include <kotavel/filesystem/filesystem.hpp>
#include <kotavel/http/api_response.hpp>

namespace kotavel::http {

enum class SameSite { lax, strict, none };
enum class SessionTransport { cookie, header };

struct SessionConfig final {
    std::string driver = "file";
    // Relative paths are resolved against the consumer application's root.
    std::filesystem::path files = "storage/framework/sessions";
    std::chrono::minutes lifetime{120};
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

class FileSessionStore final : public contracts::SessionStore {
public:
    explicit FileSessionStore(std::filesystem::path directory,
                              std::shared_ptr<contracts::Filesystem> filesystem = {});
    [[nodiscard]] std::unique_ptr<contracts::SessionStore::Lock>
    lock(std::string_view id) override;
    [[nodiscard]] std::optional<contracts::SessionRecord>
    load(std::string_view id) override;
    void save(const contracts::SessionRecord& record) override;
    void remove(std::string_view id) override;
    void collect_expired(std::int64_t now) override;

private:
    void ensure_directories(bool enforce_permissions = false);
    [[nodiscard]] std::filesystem::path record_path(std::string_view id) const;
    [[nodiscard]] std::optional<contracts::SessionRecord> read_record(
        const std::filesystem::path& path, std::string_view id, bool& present);

    std::filesystem::path directory_;
    std::filesystem::path lock_directory_;
    std::shared_ptr<contracts::Filesystem> filesystem_;
    std::mutex collection_mutex_;
    std::unique_ptr<contracts::FileCursor> collection_cursor_;
    unsigned collection_directory_ = 0; // 0: legacy root, 1..256: 00..ff.
};

namespace detail {
class SessionScope final {
public:
    struct State;
    SessionScope(contracts::SessionStore& store, contracts::SessionRecord record,
                 const SessionConfig& config);
    ~SessionScope();
    SessionScope(const SessionScope&) = delete;
    SessionScope& operator=(const SessionScope&) = delete;

    [[nodiscard]] contracts::SessionRecord& record() noexcept;
    void save();

private:
    std::unique_ptr<State> state_;
};
} // namespace detail

} // namespace kotavel::http
