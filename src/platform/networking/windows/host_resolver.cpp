#include "../../host_resolver.hpp"

#include <winsock2.h>
#include <ws2tcpip.h>

#include <string>

namespace kotavel::platform::detail {
bool resolves_host(std::string_view host) {
    static const bool winsock_ready = [] {
        WSADATA data{};
        return WSAStartup(MAKEWORD(2, 2), &data) == 0;
    }();
    if (!winsock_ready) return false;

    const std::string name(host);
    addrinfo hints{};
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_family = AF_UNSPEC;
    addrinfo* results = nullptr;
    const int status = getaddrinfo(name.c_str(), nullptr, &hints, &results);
    if (results) freeaddrinfo(results);
    return status == 0;
}
} // namespace kotavel::platform::detail
