#pragma once

#include <chrono>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <utility>

#include <kotavel/session/session.hpp>

namespace kotavel::session {

using InstanceId = http::SessionInstanceId;
using ContextId = http::SessionContextId;
using LinkGrant = http::SessionLinkGrant;
using ContextResolver = http::SessionContextResolver;

// Handle to durable application data shared by explicitly linked clients.
// Desktop can retain this object; HTTP code resolves its own context only
// after redeeming a LinkGrant against that request's independent SessionID.
class Context final {
public:
    [[nodiscard]] static Context create(std::shared_ptr<ContextResolver> resolver) {
        if (!resolver) throw std::invalid_argument("Session context requires a resolver");
        auto id = resolver->create_context();
        return Context(std::move(resolver), std::move(id));
    }

    [[nodiscard]] static Context default_for(std::shared_ptr<ContextResolver> resolver) {
        if (!resolver) throw std::invalid_argument("Session context requires a resolver");
        auto id = resolver->default_context();
        return Context(std::move(resolver), std::move(id));
    }

    Context(std::shared_ptr<ContextResolver> resolver, ContextId id)
        : resolver_(std::move(resolver)), id_(std::move(id)) {
        if (!resolver_) throw std::invalid_argument("Session context requires a resolver");
        if (id_.value.empty()) throw std::invalid_argument("Session context ID cannot be empty");
    }

    [[nodiscard]] const ContextId& id() const noexcept { return id_; }
    [[nodiscard]] LinkGrant issue_link_grant(
        std::chrono::seconds lifetime = std::chrono::seconds(60)) const {
        return resolver_->issue_link_grant(id_, lifetime);
    }
    void put(std::string_view key, http::JsonValue value,
             std::optional<std::chrono::seconds> lifetime = std::nullopt) {
        resolver_->put(id_, key, std::move(value), lifetime);
    }
    [[nodiscard]] std::optional<http::JsonValue> get(std::string_view key) const {
        return resolver_->get(id_, key);
    }
    [[nodiscard]] bool revoke() { return resolver_->delete_context(id_); }

private:
    std::shared_ptr<ContextResolver> resolver_;
    ContextId id_;
};

} // namespace kotavel::session
