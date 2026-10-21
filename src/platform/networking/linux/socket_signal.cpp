#include "../../socket_signal.hpp"

#include <sys/socket.h>

namespace kotavel::platform::detail {

void configure_socket_signal_handling(int) noexcept {}

std::ptrdiff_t send_without_sigpipe(int socket, const char* data, std::size_t size) noexcept {
    return static_cast<std::ptrdiff_t>(::send(socket, data, size, MSG_NOSIGNAL));
}

} // namespace kotavel::platform::detail
