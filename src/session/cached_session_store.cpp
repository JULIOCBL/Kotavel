#include <kotavel/session/session.hpp>

#include "session_size.hpp"

#include <algorithm>
#include <atomic>
#include <functional>
#include <mutex>
#include <shared_mutex>
#include <stdexcept>
#include <unordered_map>
#include <utility>

namespace kotavel::http {
namespace {
std::int64_t steady_nanoseconds() {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
}

std::size_t effective_shard_count(std::size_t max_sessions,
                                  std::size_t requested_shards) noexcept {
    if (max_sessions == 0) return 1;
    return std::min(max_sessions, requested_shards);
}

} // namespace

struct CachedSessionStore::State final {
    struct Entry final {
        Entry(std::shared_ptr<const contracts::SessionRecord> value, std::size_t estimated_size,
              std::int64_t loaded_at_ns)
            : record(std::move(value)), bytes(estimated_size), loaded_at(loaded_at_ns),
              last_access(loaded_at_ns) {}

        const std::shared_ptr<const contracts::SessionRecord> record;
        const std::size_t bytes;
        const std::int64_t loaded_at;
        std::atomic<std::int64_t> last_access;
    };

    struct Shard final {
        mutable std::shared_mutex mutex;
        std::unordered_map<std::string, std::shared_ptr<Entry>> entries;
        std::size_t bytes = 0;
        std::uint64_t generation = 0;
    };

    using SharedGuard = std::shared_lock<std::shared_mutex>;
    using UniqueGuard = std::unique_lock<std::shared_mutex>;

    [[nodiscard]] SharedGuard lock_shared(const Shard& shard) const {
        SharedGuard guard(shard.mutex, std::try_to_lock);
        if (!guard.owns_lock()) {
            const auto started = std::chrono::steady_clock::now();
            guard.lock();
            observe_lock_wait(started);
        }
        return guard;
    }

    [[nodiscard]] UniqueGuard lock_unique(Shard& shard) {
        UniqueGuard guard(shard.mutex, std::try_to_lock);
        if (!guard.owns_lock()) {
            const auto started = std::chrono::steady_clock::now();
            guard.lock();
            observe_lock_wait(started);
        }
        return guard;
    }

    void observe_lock_wait(std::chrono::steady_clock::time_point started) const noexcept {
        const auto waited = static_cast<std::uint64_t>(std::chrono::duration_cast<
            std::chrono::nanoseconds>(std::chrono::steady_clock::now() - started).count());
        contended_lock_acquisitions.fetch_add(1, std::memory_order_relaxed);
        lock_wait_ns.fetch_add(waited, std::memory_order_relaxed);
        auto maximum = max_lock_wait_ns.load(std::memory_order_relaxed);
        while (maximum < waited && !max_lock_wait_ns.compare_exchange_weak(
            maximum, waited, std::memory_order_relaxed)) {}
    }

    State(std::shared_ptr<contracts::SessionStore> backing_store,
          std::size_t session_limit, std::size_t byte_limit,
          std::chrono::milliseconds cache_ttl, std::size_t shard_count_value)
        : backing(std::move(backing_store)), max_sessions(session_limit),
          max_bytes(byte_limit),
          shard_count(effective_shard_count(session_limit, shard_count_value)),
          ttl_ns(std::chrono::duration_cast<std::chrono::nanoseconds>(
              cache_ttl).count()), shards(std::make_unique<Shard[]>(shard_count)) {}

    [[nodiscard]] std::size_t index(std::string_view id) const noexcept {
        return std::hash<std::string_view>{}(id) % shard_count;
    }

    [[nodiscard]] std::size_t shard_session_limit(std::size_t shard) const noexcept {
        return max_sessions / shard_count + (shard < max_sessions % shard_count ? 1 : 0);
    }

    [[nodiscard]] bool reserve_bytes(std::size_t bytes) noexcept {
        auto resident = resident_bytes.load(std::memory_order_relaxed);
        while (resident <= max_bytes && bytes <= max_bytes - resident) {
            if (resident_bytes.compare_exchange_weak(resident, resident + bytes,
                    std::memory_order_relaxed)) return true;
        }
        return false;
    }

    void release_bytes(std::size_t bytes) noexcept {
        resident_bytes.fetch_sub(bytes, std::memory_order_relaxed);
    }

    [[nodiscard]] std::uint64_t generation(std::size_t shard_index) const {
        auto& shard = shards[shard_index];
        auto guard = lock_shared(shard);
        return shard.generation;
    }

    [[nodiscard]] std::shared_ptr<const contracts::SessionRecord> find(std::string_view id) {
        const auto shard_index = index(id);
        auto& shard = shards[shard_index];
        std::shared_ptr<Entry> entry;
        {
            auto guard = lock_shared(shard);
            const auto found = shard.entries.find(std::string(id));
            if (found == shard.entries.end()) return {};
            entry = found->second;
        }
        const auto now = steady_nanoseconds();
        if (now - entry->loaded_at > ttl_ns) {
            auto guard = lock_unique(shard);
            const auto found = shard.entries.find(std::string(id));
            if (found != shard.entries.end() && found->second == entry) {
                ++shard.generation;
                shard.bytes -= entry->bytes;
                release_bytes(entry->bytes);
                shard.entries.erase(found);
                evictions.fetch_add(1, std::memory_order_relaxed);
            }
            return {};
        }
        entry->last_access.store(now, std::memory_order_relaxed);
        return entry->record;
    }

    void erase(std::string_view id) {
        const auto shard_index = index(id);
        auto& shard = shards[shard_index];
        auto guard = lock_unique(shard);
        ++shard.generation;
        const auto found = shard.entries.find(std::string(id));
        if (found == shard.entries.end()) return;
        shard.bytes -= found->second->bytes;
        release_bytes(found->second->bytes);
        shard.entries.erase(found);
    }

    void put(std::shared_ptr<const contracts::SessionRecord> record,
             std::uint64_t observed_generation) {
        if (max_sessions == 0 || max_bytes == 0) return;
        const auto shard_index = index(record->id);
        auto& shard = shards[shard_index];
        const auto bytes = detail::estimate_session_record_size(*record);
        const auto session_limit = shard_session_limit(shard_index);
        if (session_limit == 0 || bytes > max_bytes) return;

        const auto now = steady_nanoseconds();
        auto entry = std::make_shared<Entry>(std::move(record), bytes, now);
        auto guard = lock_unique(shard);
        if (shard.generation != observed_generation) return;

        const auto existing = shard.entries.find(entry->record->id);
        if (existing != shard.entries.end()) {
            if (existing->second->record->revision > entry->record->revision) return;
            shard.bytes -= existing->second->bytes;
            release_bytes(existing->second->bytes);
            shard.entries.erase(existing);
        }
        const auto evict_oldest = [&] {
            auto victim = shard.entries.end();
            auto oldest = std::numeric_limits<std::int64_t>::max();
            for (auto current = shard.entries.begin(); current != shard.entries.end(); ++current) {
                const auto accessed = current->second->last_access.load(std::memory_order_relaxed);
                if (accessed < oldest) {
                    oldest = accessed;
                    victim = current;
                }
            }
            if (victim == shard.entries.end()) return false;
            shard.bytes -= victim->second->bytes;
            release_bytes(victim->second->bytes);
            shard.entries.erase(victim);
            evictions.fetch_add(1, std::memory_order_relaxed);
            return true;
        };
        while (shard.entries.size() >= session_limit) {
            if (!evict_oldest()) return;
        }

        while (!reserve_bytes(bytes)) {
            if (!evict_oldest()) return;
        }
        shard.bytes += bytes;
        const auto entry_id = entry->record->id;
        try {
            shard.entries.emplace(entry_id, std::move(entry));
        } catch (...) {
            shard.bytes -= bytes;
            release_bytes(bytes);
            throw;
        }
    }

    std::shared_ptr<contracts::SessionStore> backing;
    std::size_t max_sessions;
    std::size_t max_bytes;
    std::size_t shard_count;
    std::int64_t ttl_ns;
    std::atomic<std::uint64_t> hits{0};
    std::atomic<std::uint64_t> misses{0};
    std::atomic<std::uint64_t> evictions{0};
    std::atomic<std::size_t> resident_bytes{0};
    mutable std::atomic<std::uint64_t> contended_lock_acquisitions{0};
    mutable std::atomic<std::uint64_t> lock_wait_ns{0};
    mutable std::atomic<std::uint64_t> max_lock_wait_ns{0};
    std::unique_ptr<Shard[]> shards;
};

CachedSessionStore::CachedSessionStore(
    std::shared_ptr<contracts::SessionStore> backing, std::size_t max_sessions,
    std::size_t max_bytes, std::chrono::milliseconds ttl, std::size_t shard_count) {
    if (!backing) throw std::invalid_argument("Cached session store requires a backing store");
    if (ttl <= std::chrono::milliseconds::zero())
        throw std::invalid_argument("Session cache TTL must be greater than zero");
    if (shard_count == 0 || shard_count > 1024)
        throw std::invalid_argument("Session cache shard count must be between 1 and 1024");
    state_ = std::make_unique<State>(std::move(backing), max_sessions, max_bytes, ttl, shard_count);
}

CachedSessionStore::~CachedSessionStore() = default;

std::unique_ptr<contracts::SessionStore::Lock>
CachedSessionStore::lock(std::string_view id) {
    return state_->backing->lock(id);
}

std::optional<contracts::SessionRecord> CachedSessionStore::load(std::string_view id) {
    auto snapshot = load_snapshot(id);
    if (!snapshot) return std::nullopt;
    return *snapshot;
}

std::shared_ptr<const contracts::SessionRecord>
CachedSessionStore::load_snapshot(std::string_view id) {
    if (auto cached = state_->find(id)) {
        state_->hits.fetch_add(1, std::memory_order_relaxed);
        return cached;
    }
    state_->misses.fetch_add(1, std::memory_order_relaxed);
    const auto shard_index = state_->index(id);
    const auto observed_generation = state_->generation(shard_index);
    std::shared_ptr<const contracts::SessionRecord> loaded;
    if (auto* snapshots = dynamic_cast<contracts::SessionSnapshotStore*>(state_->backing.get())) {
        loaded = snapshots->load_snapshot(id);
    } else if (auto record = state_->backing->load(id)) {
        loaded = std::make_shared<const contracts::SessionRecord>(std::move(*record));
    }
    if (loaded && loaded->estimated_values_bytes == 0) {
        auto normalized = std::make_shared<contracts::SessionRecord>(*loaded);
        normalized->estimated_values_bytes = detail::estimate_session_values_size(normalized->values);
        loaded = std::move(normalized);
    }
    if (loaded) state_->put(loaded, observed_generation);
    if (auto cached = state_->find(id)) return cached;
    return loaded;
}

void CachedSessionStore::save(const contracts::SessionRecord& record) {
    state_->erase(record.id);
    state_->backing->save(record);
    state_->erase(record.id);
}

void CachedSessionStore::remove(std::string_view id) {
    state_->erase(id);
    state_->backing->remove(id);
    state_->erase(id);
}

bool CachedSessionStore::commit(contracts::SessionRecord& record) {
    if (!contracts::commit_session(*state_->backing, record)) {
        state_->erase(record.id);
        return false;
    }
    state_->erase(record.id);
    return true;
}

bool CachedSessionStore::commit_rotation(
    contracts::SessionRecord& replacement, std::uint64_t expected_previous_revision,
    std::span<const std::string> retired_ids) {
    state_->erase(replacement.id);
    for (const auto& id : retired_ids) state_->erase(id);
    const bool committed = contracts::commit_session_rotation(
        *state_->backing, replacement, expected_previous_revision, retired_ids);
    state_->erase(replacement.id);
    for (const auto& id : retired_ids) state_->erase(id);
    return committed;
}

bool CachedSessionStore::commit_rebased(
    contracts::SessionRecord& record,
    const std::function<bool(contracts::SessionRecord&)>& rebase) {
    state_->erase(record.id);
    if (!contracts::commit_session_rebased(*state_->backing, record, rebase)) {
        state_->erase(record.id);
        return false;
    }
    state_->erase(record.id);
    return true;
}

void CachedSessionStore::collect_expired(std::int64_t now) {
    state_->backing->collect_expired(now);
    for (std::size_t index = 0; index < state_->shard_count; ++index) {
        auto& shard = state_->shards[index];
        auto guard = state_->lock_unique(shard);
        ++shard.generation;
        for (auto item = shard.entries.begin(); item != shard.entries.end();) {
            if (item->second->record->expires_at <= now) {
                shard.bytes -= item->second->bytes;
                state_->release_bytes(item->second->bytes);
                item = shard.entries.erase(item);
            } else ++item;
        }
        ++shard.generation;
    }
}

CachedSessionStore::Statistics CachedSessionStore::statistics() const {
    Statistics result;
    result.hits = state_->hits.load(std::memory_order_relaxed);
    result.misses = state_->misses.load(std::memory_order_relaxed);
    result.evictions = state_->evictions.load(std::memory_order_relaxed);
    result.contended_lock_acquisitions = state_->contended_lock_acquisitions.load(
        std::memory_order_relaxed);
    result.lock_wait_ns = state_->lock_wait_ns.load(std::memory_order_relaxed);
    result.max_lock_wait_ns = state_->max_lock_wait_ns.load(std::memory_order_relaxed);
    for (std::size_t index = 0; index < state_->shard_count; ++index) {
        const auto& shard = state_->shards[index];
        auto guard = state_->lock_shared(shard);
        result.resident_sessions += shard.entries.size();
    }
    result.resident_bytes = state_->resident_bytes.load(std::memory_order_relaxed);
    return result;
}

} // namespace kotavel::http
