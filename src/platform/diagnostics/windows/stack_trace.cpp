#include "../../stack_trace.hpp"

#define NOMINMAX
#include <windows.h>

#include <algorithm>
#include <cstdint>
#include <iomanip>
#include <sstream>

namespace kotavel::platform::detail {
std::size_t capture_stack_trace(std::span<void*> frames) noexcept {
    if (frames.empty()) return 0;
    const auto capacity = static_cast<DWORD>(std::min<std::size_t>(frames.size(), MAXDWORD));
    return CaptureStackBackTrace(1, capacity, frames.data(), nullptr);
}

std::string format_stack_trace(std::span<void* const> frames) {
    std::ostringstream output;
    for (std::size_t i = 0; i < frames.size(); ++i)
        output << '#' << i << " 0x" << std::hex
               << reinterpret_cast<std::uintptr_t>(frames[i]) << '\n';
    return output.str();
}
} // namespace kotavel::platform::detail
