#pragma once

#include <cstdint>
#include <functional>
#include <limits>
#include <memory>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

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
    // Optimistic concurrency revision. Zero is reserved for a new record;
    // existing custom stores keep source compatibility with the initial value.
    std::uint64_t revision = 1;
    // Derived in-memory size hint used to enforce session limits without
    // rescanning all values for every request. Zero means unknown.
    std::size_t estimated_values_bytes = 0;
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
    // Short-lived store lock used around individual load/remove operations.
    [[nodiscard]] virtual std::unique_ptr<Lock> lock(std::string_view id) = 0;
    [[nodiscard]] virtual std::optional<SessionRecord>
    load(std::string_view id) = 0;
    virtual void save(const SessionRecord& record) = 0;
    virtual void remove(std::string_view id) = 0;
    virtual void collect_expired(std::int64_t) {}
};

// Optional capability for stores that can atomically compare and advance a
// revision. Kept separate so existing SessionStore vtables remain unchanged.
class SessionRevisionStore {
public:
    virtual ~SessionRevisionStore() = default;
    [[nodiscard]] virtual bool commit(SessionRecord& record) = 0;
    [[nodiscard]] virtual bool commit_rebased(
        SessionRecord& record,
        const std::function<bool(SessionRecord&)>& rebase) = 0;
};

// Source-compatible fallback for custom SessionStore implementations. Stores
// that support atomic CAS should also implement SessionRevisionStore.
inline bool commit_session(SessionStore& store, SessionRecord& record) {
    if (auto* revision = dynamic_cast<SessionRevisionStore*>(&store))
        return revision->commit(record);
    auto guard = store.lock(record.id);
    const auto current = store.load(record.id);
    if (record.revision == 0) {
        if (current) return false;
    } else if (!current || current->revision != record.revision) {
        return false;
    }
    if (record.revision == std::numeric_limits<std::uint64_t>::max())
        throw std::overflow_error("Session revision exhausted");
    ++record.revision;
    store.save(record);
    return true;
}

inline bool commit_session_rebased(
    SessionStore& store, SessionRecord& record,
    const std::function<bool(SessionRecord&)>& rebase) {
    if (auto* revision = dynamic_cast<SessionRevisionStore*>(&store))
        return revision->commit_rebased(record, rebase);
    auto guard = store.lock(record.id);
    auto latest = store.load(record.id);
    if (!latest || latest->id != record.id || !rebase(*latest)) return false;
    if (latest->revision >= std::numeric_limits<std::uint64_t>::max())
        throw std::overflow_error("Session revision exhausted");
    ++latest->revision;
    store.save(*latest);
    record = std::move(*latest);
    return true;
}

// Optional capability for stores that can return immutable snapshots without
// copying cached records. Kept separate so existing SessionStore vtables and
// custom implementations remain compatible.
class SessionSnapshotStore {
public:
    virtual ~SessionSnapshotStore() = default;
    [[nodiscard]] virtual std::shared_ptr<const SessionRecord>
    load_snapshot(std::string_view id) = 0;
};

// Optional capability for stores that can atomically replace a session
// identity and retire the previous IDs in one durable transaction.
class SessionRotationStore {
public:
    virtual ~SessionRotationStore() = default;
    [[nodiscard]] virtual bool commit_rotation(
        SessionRecord& replacement, std::uint64_t expected_previous_revision,
        std::span<const std::string> retired_ids) = 0;
};

// Compatibility fallback for custom stores. It validates the previous
// revision while holding that store's per-ID lock, then commits and retires;
// durable stores should implement SessionRotationStore to make it atomic.
inline bool commit_session_rotation(SessionStore& store, SessionRecord& replacement,
                                    std::uint64_t expected_previous_revision,
                                    std::span<const std::string> retired_ids) {
    if (auto* rotation = dynamic_cast<SessionRotationStore*>(&store))
        return rotation->commit_rotation(replacement, expected_previous_revision, retired_ids);
    if (retired_ids.empty()) return commit_session(store, replacement);

    std::optional<SessionRecord> previous;
    {
        auto previous_guard = store.lock(retired_ids.front());
        previous = store.load(retired_ids.front());
    }
    if (expected_previous_revision == 0) {
        if (previous) return false;
    } else if (!previous || previous->revision != expected_previous_revision) {
        return false;
    }
    if (!commit_session(store, replacement)) return false;
    for (const auto& id : retired_ids) {
        auto guard = store.lock(id);
        store.remove(id);
    }
    return true;
}

} // namespace kotavel::contracts
