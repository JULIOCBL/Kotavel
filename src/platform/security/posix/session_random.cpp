#include "../../../session/session_internal.hpp"

#include <cerrno>
#include <fcntl.h>
#include <system_error>
#include <unistd.h>

namespace kotavel::http::detail {

void fill_secure_random(std::span<unsigned char> bytes) {
    const int descriptor = ::open("/dev/urandom", O_RDONLY);
    if (descriptor < 0)
        throw std::system_error(errno, std::generic_category(), "Cannot open operating-system random source");
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto count = ::read(descriptor, bytes.data() + offset, bytes.size() - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) {
            const int error = count < 0 ? errno : EIO;
            ::close(descriptor);
            throw std::system_error(error, std::generic_category(), "Cannot read operating-system random source");
        }
        offset += static_cast<std::size_t>(count);
    }
    ::close(descriptor);
}

} // namespace kotavel::http::detail
