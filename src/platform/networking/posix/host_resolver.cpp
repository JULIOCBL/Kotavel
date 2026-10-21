#include "../../host_resolver.hpp"

#include <netdb.h>
#include <sys/socket.h>

#include <string>

namespace kotavel::platform::detail {
bool resolves_host(std::string_view host) {
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
