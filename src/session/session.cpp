#include <kotavel/session/session.hpp>
#include <kotavel/env/env.hpp>

#include "session_internal.hpp"

#include <algorithm>
#include <charconv>
#include <chrono>
#include <stdexcept>
#include <utility>
#include <vector>

namespace kotavel::http {
namespace {
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

    kotavel::ConfigNode domain = nullptr;
    const auto raw_domain = env("SESSION_DOMAIN");
    if (!raw_domain.is_null()) domain = raw_domain.as_string();
    return {
        {"driver", env("SESSION_DRIVER", "file")},
        {"files", env("SESSION_FILES", "storage/framework/sessions")},
        {"lifetime", lifetime},
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
    if (configuration.contains("session.files")) config.files = configuration.get<std::string>("session.files");
    if (configuration.contains("session.lifetime")) {
        const int minutes = configuration.get<int>("session.lifetime");
        if (minutes <= 0) throw std::invalid_argument("session.lifetime must be greater than zero");
        config.lifetime = std::chrono::minutes(minutes);
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
          locked_shards{record.id.substr(0, 3)}, previous(previous_state) {}

    contracts::SessionStore* store;
    contracts::SessionRecord record;
    std::chrono::minutes lifetime;
    std::vector<std::string> retired_ids;
    std::vector<std::unique_ptr<contracts::SessionStore::Lock>> locks;
    std::vector<std::string> locked_shards;
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

SessionScope::~SessionScope() { active_session = state_->previous; }

contracts::SessionRecord& SessionScope::record() noexcept { return state_->record; }

void SessionScope::save() {
    for (const auto& id : state_->retired_ids) state_->store->remove(id);
    const auto now = now_seconds();
    state_->record.last_activity = now;
    state_->record.expires_at = now +
        std::chrono::duration_cast<std::chrono::seconds>(state_->lifetime).count();
    state_->store->save(state_->record);
}

SessionScope::State& current_state() {
    if (!active_session)
        throw std::logic_error("Session is only available while handling a web request with session middleware");
    return *active_session;
}

void rotate_session(SessionScope::State& state, bool invalidate) {
    state.retired_ids.push_back(state.record.id);
    do {
        state.record.id = generate_session_id();
    } while (std::find(state.locked_shards.begin(), state.locked_shards.end(),
                       state.record.id.substr(0, 3)) != state.locked_shards.end());
    state.locked_shards.push_back(state.record.id.substr(0, 3));
    auto lock = state.store->lock(state.record.id);
    // The lock must outlive the request. Retain it in the scope's state.
    state.locks.push_back(std::move(lock));
    state.record.created_at = now_seconds();
    state.record.last_activity = state.record.created_at;
    state.record.expires_at = state.record.created_at +
        std::chrono::duration_cast<std::chrono::seconds>(state.lifetime).count();
    if (invalidate) {
        state.record.values.clear();
    }
}
} // namespace detail

void Session::put_json(std::string_view key, JsonValue value) {
    if (key.empty()) throw std::invalid_argument("Session keys cannot be empty");
    detail::current_state().record.values.insert_or_assign(std::string(key), std::move(value));
}

JsonValue Session::get(std::string_view key, JsonValue fallback) {
    const auto& values = detail::current_state().record.values;
    const auto found = values.find(std::string(key));
    return found == values.end() ? std::move(fallback) : found->second;
}

bool Session::exists(std::string_view key) {
    return detail::current_state().record.values.contains(std::string(key));
}

bool Session::has(std::string_view key) {
    const auto& values = detail::current_state().record.values;
    const auto found = values.find(std::string(key));
    return found != values.end() && !std::holds_alternative<std::nullptr_t>(found->second.value());
}

void Session::forget(std::string_view key) {
    detail::current_state().record.values.erase(std::string(key));
}

void Session::flush() { detail::current_state().record.values.clear(); }

JsonValue::Object Session::all() { return detail::current_state().record.values; }

std::string Session::id() { return detail::current_state().record.id; }

void Session::regenerate() { detail::rotate_session(detail::current_state(), false); }

void Session::invalidate() { detail::rotate_session(detail::current_state(), true); }

Session& session() {
    static Session facade;
    (void)detail::current_state(); // Fail clearly when used outside a session-enabled request.
    return facade;
}
} // namespace kotavel::http
