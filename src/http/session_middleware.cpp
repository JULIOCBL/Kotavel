#include <kotavel/http/session_middleware.hpp>

#include "../session/session_internal.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cctype>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

namespace kotavel::http {
namespace {
std::int64_t now_seconds() {
    return std::chrono::duration_cast<std::chrono::seconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
}

bool safe_cookie_text(std::string_view value) {
    return value.find_first_of("\r\n;,") == std::string_view::npos &&
        std::none_of(value.begin(), value.end(), [](unsigned char ch) {
            return ch <= 0x20 || ch >= 0x7f;
        });
}

bool valid_header_name(std::string_view value) {
    return !value.empty() && std::all_of(value.begin(), value.end(), [](unsigned char ch) {
        return std::isalnum(ch) || std::string_view("!#$%&'*+-.^_`|~").find(ch) !=
            std::string_view::npos;
    });
}

std::optional<std::string> cookie_value(std::string_view header, std::string_view name) {
    while (!header.empty()) {
        const auto end = header.find(';');
        auto part = header.substr(0, end);
        const auto equal = part.find('=');
        if (equal != std::string_view::npos) {
            auto key = part.substr(0, equal);
            auto value = part.substr(equal + 1);
            while (!key.empty() && key.front() == ' ') key.remove_prefix(1);
            while (!key.empty() && key.back() == ' ') key.remove_suffix(1);
            if (key == name) return std::string(value);
        }
        if (end == std::string_view::npos) break;
        header.remove_prefix(end + 1);
    }
    return std::nullopt;
}

std::string serialize_cookie(const SessionConfig& config, std::string_view id) {
    std::string cookie = config.cookie + "=" + std::string(id) + "; Path=" + config.path;
    if (config.domain && !config.domain->empty()) cookie += "; Domain=" + *config.domain;
    if (!config.expire_on_close)
        cookie += "; Max-Age=" + std::to_string(
            std::chrono::duration_cast<std::chrono::seconds>(config.lifetime).count());
    if (config.http_only) cookie += "; HttpOnly";
    if (config.secure) cookie += "; Secure";
    switch (config.same_site) {
        case SameSite::lax: cookie += "; SameSite=Lax"; break;
        case SameSite::strict: cookie += "; SameSite=Strict"; break;
        case SameSite::none: cookie += "; SameSite=None"; break;
    }
    return cookie;
}

class CookieSessionIdentifierTransport final : public contracts::SessionIdentifierTransport {
public:
    explicit CookieSessionIdentifierTransport(const SessionConfig& config)
        : name_(config.cookie), prefix_(config.cookie + "="),
          suffix_(serialize_cookie(config, {}).substr(prefix_.size())) {}

    std::optional<std::string> read(const Request& request) const override {
        return cookie_value(request.header("cookie").value_or(std::string_view{}), name_);
    }

    void write(Response& response, std::string_view id) const override {
        std::string cookie;
        cookie.reserve(prefix_.size() + id.size() + suffix_.size());
        cookie.append(prefix_).append(id).append(suffix_);
        response.headers.insert_or_assign("Set-Cookie", std::move(cookie));
    }

private:
    std::string name_;
    std::string prefix_;
    std::string suffix_;
};

class HeaderSessionIdentifierTransport final : public contracts::SessionIdentifierTransport {
public:
    explicit HeaderSessionIdentifierTransport(std::string header) : header_(std::move(header)) {}

    std::optional<std::string> read(const Request& request) const override {
        const auto value = request.header(header_);
        if (!value) return std::nullopt;
        return std::string(*value);
    }

    void write(Response& response, std::string_view id) const override {
        response.headers.insert_or_assign(header_, std::string(id));
    }

private:
    std::string header_;
};

contracts::SessionRecord new_record() {
    const auto now = now_seconds();
    return {detail::generate_session_id(), now, now, now, {}};
}

struct SessionMaintenance {
    std::atomic_uint64_t requests{0};
    std::atomic_int64_t last_collection{0};
    std::atomic_flag collecting = ATOMIC_FLAG_INIT;

    void collect_if_due(contracts::SessionStore& store) {
        const auto request_number = requests.fetch_add(1, std::memory_order_relaxed);
        auto now = now_seconds();
        constexpr std::int64_t time_interval_seconds = 60;
        const auto last = last_collection.load(std::memory_order_relaxed);
        const bool request_interval_due = request_number % 100 == 0;
        const bool time_interval_due = last == 0 || now - last >= time_interval_seconds;
        if ((!request_interval_due && !time_interval_due) ||
            collecting.test_and_set(std::memory_order_acquire)) return;
        struct Release {
            std::atomic_flag& flag;
            ~Release() { flag.clear(std::memory_order_release); }
        } release{collecting};
        now = now_seconds();
        // Throttle retries after a failed sweep; the request cadence still
        // provides another retry before the next minute on active applications.
        last_collection.store(now, std::memory_order_relaxed);
        store.collect_expired(now);
    }
};
}
Middleware start_session(SessionConfig config,
                         std::shared_ptr<contracts::SessionStore> store,
                         std::shared_ptr<contracts::SessionIdentifierTransport> identifier_transport) {
    if (!store) throw std::invalid_argument("Session middleware requires a store");
    if (config.encrypt)
        throw std::invalid_argument("SESSION_ENCRYPT=true is unsupported until session encryption is implemented");
    if (config.lifetime <= std::chrono::minutes::zero())
        throw std::invalid_argument("Session lifetime must be greater than zero");
    if (config.transport != SessionTransport::cookie && config.transport != SessionTransport::header)
        throw std::invalid_argument("Session transport is not supported");

    if (!identifier_transport) {
        if (config.transport == SessionTransport::cookie) {
            if (config.cookie.empty() || !safe_cookie_text(config.cookie) ||
                !std::all_of(config.cookie.begin(), config.cookie.end(), [](unsigned char ch) {
                    return std::isalnum(ch) || std::string_view("!#$%&'*+-.^_`|~").find(ch) !=
                        std::string_view::npos;
                }))
                throw std::invalid_argument("Session cookie name is invalid");
            if (config.path.empty() || config.path.front() != '/' || !safe_cookie_text(config.path))
                throw std::invalid_argument("Session cookie path must be an absolute safe path");
            if (config.domain && !safe_cookie_text(*config.domain))
                throw std::invalid_argument("Session cookie domain is invalid");
            if (config.same_site == SameSite::none && !config.secure)
                throw std::invalid_argument("SameSite=None cookies require Secure=true");
            identifier_transport = std::make_shared<CookieSessionIdentifierTransport>(config);
        } else {
            if (!valid_header_name(config.header))
                throw std::invalid_argument("Session identifier header name is invalid");
            identifier_transport = std::make_shared<HeaderSessionIdentifierTransport>(config.header);
        }
    }

    auto maintenance = std::make_shared<SessionMaintenance>();
    return [config = std::move(config), store = std::move(store), maintenance = std::move(maintenance),
            identifier_transport = std::move(identifier_transport)]
        (const Request& request, const Next& next) -> Response {
        // Each middleware owns its maintenance cadence. Concurrent workers never
        // queue redundant sweeps while one is already scanning this store.
        maintenance->collect_if_due(*store);
        auto client_id = identifier_transport->read(request);
        if (client_id && !detail::valid_session_id(*client_id)) client_id.reset();
        auto record = client_id ? contracts::SessionRecord{} : new_record();
        if (client_id) {
            auto lock = store->lock(*client_id);
            auto loaded = store->load(*client_id);
            if (loaded && loaded->expires_at > now_seconds()) {
                record = std::move(*loaded);
            } else {
                store->remove(*client_id);
                lock.reset();
                record = new_record();
                lock = store->lock(record.id);
                auto scope = detail::SessionScope(*store, std::move(record), config);
                auto response = next();
                scope.save();
                identifier_transport->write(response, scope.record().id);
                return response;
            }
            auto scope = detail::SessionScope(*store, std::move(record), config);
            // Keep the initial identifier's lock through downstream code and persistence.
            // It is captured here to make its lifetime explicit in this scope.
            auto response = next();
            scope.save();
            identifier_transport->write(response, scope.record().id);
            return response;
        }

        auto lock = store->lock(record.id);
        auto scope = detail::SessionScope(*store, std::move(record), config);
        auto response = next();
        scope.save();
        identifier_transport->write(response, scope.record().id);
        return response;
    };
}

Middleware start_file_session(SessionConfig config,
                              const std::filesystem::path& application_root,
                              std::shared_ptr<contracts::SessionIdentifierTransport> identifier_transport) {
    if (config.driver != "file")
        throw std::invalid_argument("The configured session driver is not available in this build: " + config.driver);
    auto directory = config.files.is_absolute() ? config.files : application_root / config.files;
    return start_session(std::move(config), std::make_shared<FileSessionStore>(std::move(directory)),
                         std::move(identifier_transport));
}

} // namespace kotavel::http
