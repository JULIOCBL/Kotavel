#pragma once

#include <cstddef>

namespace kotavel::platform::detail {

void configure_socket_signal_handling(int socket) noexcept;
std::ptrdiff_t send_without_sigpipe(int socket, const char* data, std::size_t size) noexcept;

} // namespace kotavel::platform::detail
