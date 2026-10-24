#pragma once

#include <filesystem>
#include <memory>

#include <kotavel/contracts/session_identifier_transport.hpp>
#include <kotavel/contracts/session_store.hpp>
#include <kotavel/session/session.hpp>

namespace kotavel::http {

// Persists request-scoped session data and returns its identifier through the
// configured transport. Install it in the desired middleware group; APIs stay
// stateless unless the application adds session middleware there.
[[nodiscard]] Middleware start_session(
    SessionConfig config, std::shared_ptr<contracts::SessionStore> store,
    std::shared_ptr<contracts::SessionIdentifierTransport> identifier_transport = {});
[[nodiscard]] Middleware start_configured_session(
    SessionConfig config, const std::filesystem::path& application_root,
    std::shared_ptr<contracts::SessionIdentifierTransport> identifier_transport = {});

} // namespace kotavel::http
