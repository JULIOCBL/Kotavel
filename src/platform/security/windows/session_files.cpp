#include "../../../session/session_internal.hpp"

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <bcrypt.h>

#include <algorithm>
#include <limits>
#include <system_error>

namespace kotavel::http::detail {

void fill_secure_random(std::span<unsigned char> bytes) {
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto count = static_cast<ULONG>(std::min<std::size_t>(
            bytes.size() - offset,
            static_cast<std::size_t>(std::numeric_limits<ULONG>::max())));
        const auto status = ::BCryptGenRandom(nullptr, bytes.data() + offset, count,
                                               BCRYPT_USE_SYSTEM_PREFERRED_RNG);
        if (status < 0)
            throw std::system_error(static_cast<int>(status), std::system_category(),
                                    "Cannot read operating-system random source");
        offset += count;
    }
}

} // namespace kotavel::http::detail
