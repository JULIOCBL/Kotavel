#include "../stack_trace.hpp"

#include "../../diagnostics/console_text.hpp"

#ifdef KOTAVEL_HAS_BACKTRACE
#include <execinfo.h>
#endif

#include <cstdlib>
#include <memory>
#include <sstream>
#include <string_view>

namespace kotavel::platform::detail {
std::size_t capture_stack_trace(std::span<void*> frames) noexcept {
#ifdef KOTAVEL_HAS_BACKTRACE
    const auto count = ::backtrace(frames.data(), static_cast<int>(frames.size()));
    return count > 0 ? static_cast<std::size_t>(count) : 0;
#else
    (void)frames;
    return 0;
#endif
}

std::string format_stack_trace(std::span<void* const> frames) {
    std::ostringstream output;
#ifdef KOTAVEL_HAS_BACKTRACE
    std::unique_ptr<char*, decltype(&std::free)> symbols(
        ::backtrace_symbols(frames.data(), static_cast<int>(frames.size())), &std::free);
#endif
    for (std::size_t i = 0; i < frames.size(); ++i) {
        output << '#' << i << ' ';
#ifdef KOTAVEL_HAS_BACKTRACE
        if (symbols)
            output << kotavel::detail::console_safe(
                std::string_view(symbols.get()[i]).substr(0, 256));
        else
#endif
            output << frames[i];
        output << '\n';
    }
    return output.str();
}
} // namespace kotavel::platform::detail
