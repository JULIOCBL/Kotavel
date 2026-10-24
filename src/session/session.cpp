#include <kotavel/session/session.hpp>
#include <kotavel/env/env.hpp>

#include "session_internal.hpp"
#include "session_size.hpp"

#include <algorithm>
#include <charconv>
#include <chrono>
#include <map>
#include <limits>
#include <optional>
#include <set>
#include <stdexcept>
#include <utility>
#include <vector>

namespace kotavel::http {
namespace detail {
bool valid_session_id(std::string_view id) noexcept {
    if (id.size() != 64) return false;
    for (const unsigned char ch : id) {
        if (!((ch >= '0' && ch <= '9') || (ch >= 'a' && ch <= 'f'))) return false;
    }
    return true;
}

std::string generate_session_id() {
    unsigned char bytes[32]{};
    fill_secure_random(bytes);
    constexpr char digits[] = "0123456789abcdef";
    std::string result(sizeof(bytes) * 2, '0');
    for (std::size_t index = 0; index < sizeof(bytes); ++index) {
        result[index * 2] = digits[bytes[index] >> 4];
        result[index * 2 + 1] = digits[bytes[index] & 0x0f];
    }
    return result;
}
} // namespace detail

namespace {
constexpr std::string_view csrf_session_key = "_kotavel.csrf_token";

bool is_csrf_session_key(std::string_view key) noexcept {
    return key == csrf_session_key;
}

bool constant_time_equal(std::string_view expected, std::string_view candidate) noexcept {
    if (expected.size() != candidate.size()) return false;
    unsigned char difference = 0;
    for (std::size_t index = 0; index < expected.size(); ++index)
        difference |= static_cast<unsigned char>(expected[index] ^ candidate[index]);
    return difference == 0;
}

std::int64_t now_seconds() {
    return std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
}
}

kotavel::ConfigMap SessionConfig::load() {
    using kotavel::env;
    const auto raw_lifetime = env("SESSION_LIFETIME", "120");
    int lifetime = 0;
    const auto [end, error] = std::from_chars(raw_lifetime.data(),
        raw_lifetime.data() + raw_lifetime.size(), lifetime);
    if (error != std::errc{} || end != raw_lifetime.data() + raw_lifetime.size() || lifetime <= 0)
        throw std::invalid_argument("SESSION_LIFETIME must be a positive integer number of minutes");

    const auto read_integer = [](std::string_view key, const char* fallback, bool allow_zero) {
        const auto raw = env(key, fallback);
        int value = 0;
        const auto [position, parse_error] = std::from_chars(raw.data(), raw.data() + raw.size(), value);
        if (parse_error != std::errc{} || position != raw.data() + raw.size() ||
            value < 0 || (!allow_zero && value == 0))
            throw std::invalid_argument(std::string(key) + " has an invalid integer value");
        return value;
    };
    const int cache_max_sessions = read_integer("SESSION_CACHE_MAX_SESSIONS", "2048", true);
    const int cache_max_bytes = read_integer("SESSION_CACHE_MAX_BYTES", "33554432", true);
    const int cache_shards = read_integer("SESSION_CACHE_SHARDS", "64", false);
    if (cache_shards > 1024)
        throw std::invalid_argument("SESSION_CACHE_SHARDS cannot exceed 1024");
    const int cache_ttl_ms = read_integer("SESSION_CACHE_TTL_MS", "1000", false);
    const int max_lifetime = read_integer("SESSION_MAX_LIFETIME", "1440", true);
    const int max_session_bytes = read_integer("SESSION_MAX_BYTES", "1048576", false);
    const int read_connections = read_integer("SESSION_READ_CONNECTIONS", "4", false);
    const int write_queue_capacity = read_integer("SESSION_WRITE_QUEUE_CAPACITY", "1024", false);
    const int write_batch_window_us = read_integer("SESSION_WRITE_BATCH_WINDOW_US", "0", true);
    if (write_batch_window_us > 10000)
        throw std::invalid_argument("SESSION_WRITE_BATCH_WINDOW_US cannot exceed 10000");

    kotavel::ConfigNode domain = nullptr;
    const auto raw_domain = env("SESSION_DOMAIN");
    if (!raw_domain.is_null()) domain = raw_domain.as_string();
    return {
        {"driver", env("SESSION_DRIVER", "sqlite")},
        {"database", env("SESSION_DATABASE", "storage/framework/sessions.sqlite3")},
        {"lifetime", lifetime},
        {"max_lifetime", max_lifetime},
        {"cache_max_sessions", cache_max_sessions},
        {"cache_max_bytes", cache_max_bytes},
        {"cache_shards", cache_shards},
        {"cache_ttl_ms", cache_ttl_ms},
        {"max_session_bytes", max_session_bytes},
        {"sqlite_read_connections", read_connections},
        {"sqlite_write_queue_capacity", write_queue_capacity},
        {"sqlite_write_batch_window_us", write_batch_window_us},
        {"cookie", env("SESSION_COOKIE", "kotavel_session")},
        {"transport", env("SESSION_TRANSPORT", "cookie")},
        {"header", env("SESSION_HEADER", "X-Kotavel-Session")},
        {"path", env("SESSION_PATH", "/")},
        {"domain", std::move(domain)},
        {"secure", env("SESSION_SECURE_COOKIE", false)},
        {"http_only", env("SESSION_HTTP_ONLY", true)},
        {"expire_on_close", env("SESSION_EXPIRE_ON_CLOSE", false)},
        {"encrypt", env("SESSION_ENCRYPT", false)},
        {"same_site", env("SESSION_SAME_SITE", "lax")},
    };
}

SessionConfig SessionConfig::from_config(const kotavel::Config& configuration) {
    SessionConfig config;
    if (configuration.contains("session.driver")) config.driver = configuration.get<std::string>("session.driver");
    if (configuration.contains("session.database"))
        config.database = configuration.get<std::string>("session.database");
    if (configuration.contains("session.lifetime")) {
        const int minutes = configuration.get<int>("session.lifetime");
        if (minutes <= 0) throw std::invalid_argument("session.lifetime must be greater than zero");
        config.lifetime = std::chrono::minutes(minutes);
    }
    if (configuration.contains("session.max_lifetime")) {
        const int minutes = configuration.get<int>("session.max_lifetime");
        if (minutes < 0) throw std::invalid_argument("session.max_lifetime cannot be negative");
        config.max_lifetime = std::chrono::minutes(minutes);
    }
    if (configuration.contains("session.cache_max_sessions")) {
        const int value = configuration.get<int>("session.cache_max_sessions");
        if (value < 0) throw std::invalid_argument("session.cache_max_sessions cannot be negative");
        config.cache_max_sessions = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.cache_max_bytes")) {
        const int value = configuration.get<int>("session.cache_max_bytes");
        if (value < 0) throw std::invalid_argument("session.cache_max_bytes cannot be negative");
        config.cache_max_bytes = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.cache_shards")) {
        const int value = configuration.get<int>("session.cache_shards");
        if (value <= 0 || value > 1024)
            throw std::invalid_argument("session.cache_shards must be between 1 and 1024");
        config.cache_shards = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.cache_ttl_ms")) {
        const int value = configuration.get<int>("session.cache_ttl_ms");
        if (value <= 0) throw std::invalid_argument("session.cache_ttl_ms must be greater than zero");
        config.cache_ttl = std::chrono::milliseconds(value);
    }
    if (configuration.contains("session.max_session_bytes")) {
        const int value = configuration.get<int>("session.max_session_bytes");
        if (value <= 0) throw std::invalid_argument("session.max_session_bytes must be greater than zero");
        config.max_session_bytes = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.sqlite_read_connections")) {
        const int value = configuration.get<int>("session.sqlite_read_connections");
        if (value <= 0)
            throw std::invalid_argument("session.sqlite_read_connections must be greater than zero");
        config.sqlite_read_connections = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.sqlite_write_queue_capacity")) {
        const int value = configuration.get<int>("session.sqlite_write_queue_capacity");
        if (value <= 0)
            throw std::invalid_argument("session.sqlite_write_queue_capacity must be greater than zero");
        config.sqlite_write_queue_capacity = static_cast<std::size_t>(value);
    }
    if (configuration.contains("session.sqlite_write_batch_window_us")) {
        const int value = configuration.get<int>("session.sqlite_write_batch_window_us");
        if (value < 0 || value > 10000)
            throw std::invalid_argument("session.sqlite_write_batch_window_us must be between 0 and 10000");
        config.sqlite_write_batch_window = std::chrono::microseconds(value);
    }
    if (configuration.contains("session.cookie")) config.cookie = configuration.get<std::string>("session.cookie");
    if (configuration.contains("session.transport")) {
        const auto& transport = configuration.get<std::string>("session.transport");
        if (transport == "cookie") config.transport = SessionTransport::cookie;
        else if (transport == "header") config.transport = SessionTransport::header;
        else throw std::invalid_argument("session.transport must be cookie or header");
    }
    if (configuration.contains("session.header")) config.header = configuration.get<std::string>("session.header");
    if (configuration.contains("session.path")) config.path = configuration.get<std::string>("session.path");
    if (const auto* domain = configuration.find("session.domain")) {
        if (const auto* value = std::get_if<std::string>(domain)) config.domain = *value;
    }
    if (configuration.contains("session.secure")) config.secure = configuration.get<bool>("session.secure");
    if (configuration.contains("session.http_only")) config.http_only = configuration.get<bool>("session.http_only");
    if (configuration.contains("session.expire_on_close"))
        config.expire_on_close = configuration.get<bool>("session.expire_on_close");
    if (configuration.contains("session.encrypt")) config.encrypt = configuration.get<bool>("session.encrypt");
    if (configuration.contains("session.same_site")) {
        const auto& same_site = configuration.get<std::string>("session.same_site");
        if (same_site == "lax") config.same_site = SameSite::lax;
        else if (same_site == "strict") config.same_site = SameSite::strict;
        else if (same_site == "none") config.same_site = SameSite::none;
        else throw std::invalid_argument("session.same_site must be lax, strict or none");
    }
    return config;
}

namespace detail {
struct SessionScope::State final {
    State(contracts::SessionStore& session_store, contracts::SessionRecord session_record,
          const SessionConfig& session_config, State* previous_state)
        : store(&session_store), record(std::move(session_record)), lifetime(session_config.lifetime),
          max_lifetime(session_config.max_lifetime),
          max_session_bytes(session_config.max_session_bytes),
          original_id(record.id), original_revision(record.revision),
          estimated_values_size(record.estimated_values_bytes),
          dirty(record.revision == 0), previous(previous_state) {
        if (record.estimated_values_bytes == 0)
            estimated_values_size = estimate_session_values_size(record.values);
        base_values = std::make_shared<const JsonValue::Object>(std::move(record.values));
        record.values.clear();
        initialize();
    }

    State(contracts::SessionStore& session_store,
          std::shared_ptr<const contracts::SessionRecord> snapshot,
          const SessionConfig& session_config, State* previous_state)
        : store(&session_store),
          record{snapshot ? snapshot->id : std::string{},
                 snapshot ? snapshot->created_at : 0,
                 snapshot ? snapshot->last_activity : 0,
                 snapshot ? snapshot->expires_at : 0,
                 {}, snapshot ? snapshot->revision : 0,
                 snapshot ? snapshot->estimated_values_bytes : 0},
          base_values(snapshot, snapshot ? &snapshot->values : nullptr),
          lifetime(session_config.lifetime), max_lifetime(session_config.max_lifetime),
          max_session_bytes(session_config.max_session_bytes), original_id(record.id),
          original_revision(record.revision),
          estimated_values_size(record.estimated_values_bytes),
          dirty(record.revision == 0), previous(previous_state) {
        if (!snapshot) throw std::invalid_argument("Session snapshot cannot be null");
        if (record.estimated_values_bytes == 0)
            estimated_values_size = estimate_session_values_size(*base_values);
        initialize();
    }

    void initialize() {
        if (estimated_values_size > max_session_bytes)
            throw std::length_error("Session data exceeds the configured size limit");
        record.estimated_values_bytes = estimated_values_size;
        if (max_lifetime > std::chrono::minutes::zero()) {
            const auto maximum_age =
                std::chrono::duration_cast<std::chrono::seconds>(max_lifetime).count();
            const auto absolute_expiry = record.created_at + maximum_age;
            if (record.expires_at > absolute_expiry) {
                record.expires_at = absolute_expiry;
                dirty = true;
            }
        }
    }

    [[nodiscard]] const JsonValue* current_value(std::string_view key) const {
        const auto changed = changes.find(key);
        if (changed != changes.end()) return changed->second ? &*changed->second : nullptr;
        const auto found = base_values->find(std::string(key));
        return found == base_values->end() ? nullptr : &found->second;
    }

    [[nodiscard]] JsonValue::Object materialize_values() const {
        JsonValue::Object values = *base_values;
        for (const auto& [key, value] : changes) {
            if (value) values.insert_or_assign(key, *value);
            else values.erase(key);
        }
        return values;
    }

    void replace_base_values(JsonValue::Object values) {
        base_values = std::make_shared<const JsonValue::Object>(std::move(values));
        changes.clear();
        original_values.clear();
        record.values.clear();
    }

    void remember_original(std::string_view key) {
        if (original_values.contains(key)) return;
        const auto* found = current_value(key);
        if (!found) original_values.emplace(std::string(key), std::nullopt);
        else original_values.emplace(std::string(key), *found);
    }

    [[nodiscard]] std::size_t size_after_replacing(std::string_view key,
                                                    const JsonValue& value) const {
        return estimate_session_values_replacing(
            estimated_values_size, key, current_value(key), value);
    }

    void set_estimated_values_size(std::size_t size) noexcept {
        estimated_values_size = size;
        record.estimated_values_bytes = size;
    }

    void subtract_value_size(std::string_view key) {
        set_estimated_values_size(estimate_session_values_erasing(
            estimated_values_size, key, current_value(key)));
    }

    contracts::SessionStore* store;
    contracts::SessionRecord record;
    std::shared_ptr<const JsonValue::Object> base_values;
    std::map<std::string, std::optional<JsonValue>, std::less<>> changes;
    std::chrono::minutes lifetime;
    std::chrono::minutes max_lifetime;
    std::size_t max_session_bytes;
    std::string original_id;
    std::uint64_t original_revision;
    std::size_t estimated_values_size;
    std::map<std::string, std::optional<JsonValue>, std::less<>> original_values;
    std::map<std::string, std::uint64_t, std::less<>> increments;
    std::set<std::string, std::less<>> non_rebasable_keys;
    std::vector<std::string> retired_ids;
    bool dirty = false;
    bool exclusive_write = false;
    State* previous = nullptr;
};

namespace {
thread_local SessionScope::State* active_session = nullptr;
}

SessionScope::SessionScope(contracts::SessionStore& store,
                           contracts::SessionRecord record,
                           const SessionConfig& config)
    : state_(std::make_unique<State>(store, std::move(record), config,
                                     active_session)) {
    active_session = state_.get();
}

SessionScope::SessionScope(
    contracts::SessionStore& store,
    std::shared_ptr<const contracts::SessionRecord> record,
    const SessionConfig& config)
    : state_(std::make_unique<State>(store, std::move(record), config,
                                     active_session)) {
    active_session = state_.get();
}

SessionScope::~SessionScope() { active_session = state_->previous; }

contracts::SessionRecord& SessionScope::record() noexcept { return state_->record; }

void SessionScope::save() { (void)try_save(); }

bool SessionScope::try_save() {
    const auto now = now_seconds();
    if (state_->max_lifetime > std::chrono::minutes::zero()) {
        const auto maximum_age =
            std::chrono::duration_cast<std::chrono::seconds>(state_->max_lifetime).count();
        if (now >= state_->record.created_at &&
            now - state_->record.created_at >= maximum_age) return false;
    }
    const auto lifetime_seconds =
        std::chrono::duration_cast<std::chrono::seconds>(state_->lifetime).count();
    const auto activity_interval = std::max<std::int64_t>(1,
        std::min<std::int64_t>(60, lifetime_seconds / 2));
    const bool touch_due = now - state_->record.last_activity >= activity_interval;
    if (!state_->dirty && !touch_due) return true;

    const auto delta = state_->changes;

    const auto set_expiry = [now, lifetime_seconds](contracts::SessionRecord& record,
                                                    std::chrono::minutes max_lifetime) {
        record.last_activity = now;
        record.expires_at = now + lifetime_seconds;
        if (max_lifetime > std::chrono::minutes::zero()) {
            const auto maximum_age =
                std::chrono::duration_cast<std::chrono::seconds>(max_lifetime).count();
            record.expires_at = std::min(record.expires_at, record.created_at + maximum_age);
        }
    };

    set_expiry(state_->record, state_->max_lifetime);
    // Materialize the immutable snapshot only when a request must persist.
    state_->record.values = state_->materialize_values();
    const bool rotated = state_->record.id != state_->original_id ||
        !state_->retired_ids.empty();
    // Identity changes validate the previous revision and replace/remove IDs
    // atomically when the backing store supports it. New sessions and exclusive
    // writes retain strict CAS semantics; ordinary updates use disjoint-key merge.
    if (rotated) {
        if (!contracts::commit_session_rotation(*state_->store, state_->record,
                state_->original_revision, state_->retired_ids)) return false;
    } else if (state_->original_revision == 0 || state_->exclusive_write) {
        if (!contracts::commit_session(*state_->store, state_->record)) return false;
    } else {
        const auto original_values = state_->original_values;
        const auto increments = state_->increments;
        const auto non_rebasable_keys = state_->non_rebasable_keys;
        const auto desired_activity = state_->record.last_activity;
        const auto desired_expiry = state_->record.expires_at;
        const auto maximum_size = state_->max_session_bytes;
        const auto merge = [now, original_values, delta, increments, non_rebasable_keys,
                            desired_activity, desired_expiry, maximum_size]
            (contracts::SessionRecord& latest) {
                if (latest.expires_at <= now) return false;
                if (latest.estimated_values_bytes == 0)
                    latest.estimated_values_bytes = estimate_session_values_size(latest.values);
                for (const auto& [key, _] : delta) {
                    if (increments.contains(key) && !non_rebasable_keys.contains(key)) continue;
                    const auto original = original_values.find(key);
                    const auto current = latest.values.find(key);
                    const bool original_exists = original != original_values.end() &&
                        original->second.has_value();
                    const bool current_exists = current != latest.values.end();
                    if (original_exists != current_exists ||
                        (original_exists && !(*original->second == current->second))) return false;
                }
                for (const auto& [key, value] : delta) {
                    const auto increment = increments.find(key);
                    if (increment != increments.end() &&
                        !non_rebasable_keys.contains(key)) {
                        std::uint64_t current = 0;
                        const auto found = latest.values.find(key);
                        if (found != latest.values.end()) {
                            if (const auto* number = std::get_if<std::uint64_t>(&found->second.value()))
                                current = *number;
                            else if (const auto* signed_number =
                                         std::get_if<std::int64_t>(&found->second.value());
                                     signed_number && *signed_number >= 0)
                                current = static_cast<std::uint64_t>(*signed_number);
                            else return false;
                        }
                        if (increment->second > std::numeric_limits<std::uint64_t>::max() - current)
                            throw std::overflow_error("Session counter overflow");
                        JsonValue replacement(current + increment->second);
                        latest.estimated_values_bytes = estimate_session_values_replacing(
                            latest.estimated_values_bytes, latest.values, key, replacement);
                        latest.values.insert_or_assign(key, std::move(replacement));
                    } else if (value) {
                        latest.estimated_values_bytes = estimate_session_values_replacing(
                            latest.estimated_values_bytes, latest.values, key, *value);
                        latest.values.insert_or_assign(key, *value);
                    } else {
                        latest.estimated_values_bytes = estimate_session_values_erasing(
                            latest.estimated_values_bytes, latest.values, key);
                        latest.values.erase(key);
                    }
                }
                if (latest.estimated_values_bytes > maximum_size)
                    throw std::length_error("Merged session data exceeds the configured size limit");
                latest.last_activity = desired_activity;
                latest.expires_at = desired_expiry;
                return true;
            };
        // Keep the single-statement CAS fast path for uncontended requests;
        // only stale snapshots pay for the transactional read/merge operation.
        if (!contracts::commit_session(*state_->store, state_->record) &&
            !contracts::commit_session_rebased(*state_->store, state_->record, merge))
            return false;
    }

    return true;
}

SessionScope::State& current_state() {
    if (!active_session)
        throw std::logic_error("Session is only available while handling a web request with session middleware");
    return *active_session;
}

void rotate_session(SessionScope::State& state, bool invalidate) {
    state.retired_ids.push_back(state.record.id);
    state.record.id = generate_session_id();
    state.record.revision = 0;
    state.record.created_at = now_seconds();
    state.record.last_activity = state.record.created_at;
    state.record.expires_at = state.record.created_at +
        std::chrono::duration_cast<std::chrono::seconds>(state.lifetime).count();
    if (state.max_lifetime > std::chrono::minutes::zero()) {
        state.record.expires_at = std::min(state.record.expires_at,
            state.record.created_at +
                std::chrono::duration_cast<std::chrono::seconds>(state.max_lifetime).count());
    }
    if (invalidate) {
        state.replace_base_values({});
        state.set_estimated_values_size(estimate_session_values_size(*state.base_values));
    } else {
        auto values = state.materialize_values();
        const auto key = std::string(csrf_session_key);
        const auto token = values.find(key);
        if (token != values.end()) {
            state.set_estimated_values_size(estimate_session_values_erasing(
                state.estimated_values_size, key, &token->second));
            values.erase(token);
        }
        state.replace_base_values(std::move(values));
    }
    state.dirty = true;
    state.exclusive_write = true;
}
} // namespace detail

void Session::put_json(std::string_view key, JsonValue value) {
    if (key.empty()) throw std::invalid_argument("Session keys cannot be empty");
    if (is_csrf_session_key(key))
        throw std::invalid_argument("The session CSRF token is reserved by Kotavel");
    auto& state = detail::current_state();
    const auto estimated_size = state.size_after_replacing(key, value);
    if (estimated_size > state.max_session_bytes)
        throw std::length_error("Session data exceeds the configured size limit");
    state.remember_original(key);
    state.non_rebasable_keys.insert(std::string(key));
    state.increments.erase(std::string(key));
    state.changes.insert_or_assign(std::string(key), std::move(value));
    state.set_estimated_values_size(estimated_size);
    state.dirty = true;
}

JsonValue Session::get(std::string_view key, JsonValue fallback) {
    if (is_csrf_session_key(key)) return fallback;
    const auto* found = detail::current_state().current_value(key);
    return found ? *found : std::move(fallback);
}

bool Session::exists(std::string_view key) {
    if (is_csrf_session_key(key)) return false;
    return detail::current_state().current_value(key) != nullptr;
}

std::string Session::csrf_token() {
    auto& state = detail::current_state();
    const auto key = std::string(csrf_session_key);
    if (const auto* found = state.current_value(key)) {
        if (const auto* token = std::get_if<std::string>(&found->value());
            token && detail::valid_session_id(*token)) return *token;
    }

    auto token = detail::generate_session_id();
    const JsonValue value(token);
    const auto estimated_size = state.size_after_replacing(key, value);
    if (estimated_size > state.max_session_bytes)
        throw std::length_error("Session data exceeds the configured size limit");
    state.remember_original(key);
    state.changes.insert_or_assign(key, value);
    state.set_estimated_values_size(estimated_size);
    state.non_rebasable_keys.insert(key);
    state.dirty = true;
    return token;
}

bool Session::verify_csrf_token(std::string_view candidate) {
    if (!detail::valid_session_id(candidate)) return false;
    const auto* found = detail::current_state().current_value(csrf_session_key);
    if (!found) return false;
    const auto* expected = std::get_if<std::string>(&found->value());
    return expected && constant_time_equal(*expected, candidate);
}

void Session::increment(std::string_view key, std::uint64_t amount) {
    if (key.empty()) throw std::invalid_argument("Session keys cannot be empty");
    if (is_csrf_session_key(key))
        throw std::invalid_argument("The session CSRF token is reserved by Kotavel");
    auto& state = detail::current_state();
    const std::string owned_key(key);
    std::uint64_t current = 0;
    if (const auto* found = state.current_value(owned_key)) {
        if (const auto* number = std::get_if<std::uint64_t>(&found->value())) current = *number;
        else if (const auto* signed_number = std::get_if<std::int64_t>(&found->value());
                 signed_number && *signed_number >= 0)
            current = static_cast<std::uint64_t>(*signed_number);
        else throw std::invalid_argument("Session counter must be a nonnegative integer");
    }
    if (amount > std::numeric_limits<std::uint64_t>::max() - current)
        throw std::overflow_error("Session counter overflow");
    if (amount == 0) return;
    const JsonValue replacement(current + amount);
    const auto estimated_size = state.size_after_replacing(key, replacement);
    if (estimated_size > state.max_session_bytes)
        throw std::length_error("Session data exceeds the configured size limit");
    auto& accumulated = state.increments[owned_key];
    if (amount > std::numeric_limits<std::uint64_t>::max() - accumulated)
        throw std::overflow_error("Session counter increment overflow");
    accumulated += amount;
    state.remember_original(key);
    state.changes.insert_or_assign(owned_key, replacement);
    state.set_estimated_values_size(estimated_size);
    state.dirty = true;
}

bool Session::has(std::string_view key) {
    if (is_csrf_session_key(key)) return false;
    const auto* found = detail::current_state().current_value(key);
    return found && !std::holds_alternative<std::nullptr_t>(found->value());
}

void Session::forget(std::string_view key) {
    if (is_csrf_session_key(key))
        throw std::invalid_argument("The session CSRF token is reserved by Kotavel");
    auto& state = detail::current_state();
    const std::string owned_key(key);
    state.increments.erase(owned_key);
    if (state.current_value(owned_key)) {
        state.remember_original(key);
        state.subtract_value_size(key);
        state.changes.insert_or_assign(owned_key, std::nullopt);
        state.non_rebasable_keys.insert(owned_key);
        state.dirty = true;
    }
}

void Session::flush() {
    auto& state = detail::current_state();
    state.replace_base_values({});
    state.set_estimated_values_size(detail::estimate_session_values_size(*state.base_values));
    state.increments.clear();
    state.dirty = true;
    state.exclusive_write = true;
}

JsonValue::Object Session::all() {
    auto values = detail::current_state().materialize_values();
    values.erase(std::string(csrf_session_key));
    return values;
}

std::string Session::id() { return detail::current_state().record.id; }

void Session::regenerate() { detail::rotate_session(detail::current_state(), false); }

void Session::invalidate() { detail::rotate_session(detail::current_state(), true); }

Session& session() {
    static Session facade;
    (void)detail::current_state(); // Fail clearly when used outside a session-enabled request.
    return facade;
}
} // namespace kotavel::http
