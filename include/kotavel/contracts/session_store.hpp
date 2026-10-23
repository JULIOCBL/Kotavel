#pragma once

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include <kotavel/http/api_response.hpp>

namespace kotavel::contracts {

// Persisted session record. Framework timestamps and application values are
// separate so Session::all() never exposes storage metadata.
struct SessionRecord final {
    std::string id;
    std::int64_t created_at = 0;
    std::int64_t last_activity = 0;
    std::int64_t expires_at = 0;
    http::JsonValue::Object values;
};

// Storage boundary. Implementations may later use another process or language
// without changing the public Session API.
class SessionStore {
public:
    virtual ~SessionStore() = default;
    class Lock {
    public:
        virtual ~Lock() = default;
    };
    // The lock spans load, request handling, and save to prevent concurrent
    // requests from silently overwriting each other's session changes.
    [[nodiscard]] virtual std::unique_ptr<Lock> lock(std::string_view id) = 0;
    [[nodiscard]] virtual std::optional<SessionRecord>
    load(std::string_view id) = 0;
    virtual void save(const SessionRecord& record) = 0;
    virtual void remove(std::string_view id) = 0;
    virtual void collect_expired(std::int64_t) {}
};

} // namespace kotavel::contracts
