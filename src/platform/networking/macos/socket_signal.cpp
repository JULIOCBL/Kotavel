#include "../../socket_signal.hpp"

#include <sys/socket.h>

namespace kotavel::platform::detail {

void configure_socket_signal_handling(int socket) noexcept {
    const int enabled = 1;
    (void)::setsockopt(socket, SOL_SOCKET, SO_NOSIGPIPE, &enabled, sizeof(enabled));
}

std::ptrdiff_t send_without_sigpipe(int socket, const char* data, std::size_t size) noexcept {
    return static_cast<std::ptrdiff_t>(::send(socket, data, size, 0));
}

} // namespace kotavel::platform::detail
