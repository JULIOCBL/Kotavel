#pragma once

#include <optional>
#include <string>
#include <string_view>

namespace kotavel::http {
class Request;
struct Response;
}

namespace kotavel::contracts {

// Boundary for carrying a session identifier over an HTTP request/response.
// Applications can provide another implementation without changing session
// storage, validation, or request-scoped state handling.
class SessionIdentifierTransport {
public:
    virtual ~SessionIdentifierTransport() = default;

    [[nodiscard]] virtual std::optional<std::string>
    read(const http::Request& request) const = 0;
    virtual void write(http::Response& response, std::string_view id) const = 0;
};

} // namespace kotavel::contracts
