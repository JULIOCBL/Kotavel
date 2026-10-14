#include <kotavel/diagnostics/error.hpp>
#include "console_text.hpp"
#include <algorithm>
#include <cstdlib>
#include <memory>
#include <sstream>
#if defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#elif defined(KOTAVEL_HAS_BACKTRACE)
#include <execinfo.h>
#endif

namespace kotavel {
// Captura nativa y formato de trazas; el control de captura es local al hilo.
namespace { thread_local bool capture_enabled = true; }
TraceCaptureScope::TraceCaptureScope(bool enabled) noexcept : previous_(capture_enabled) {
    capture_enabled = enabled;
}
TraceCaptureScope::~TraceCaptureScope() { capture_enabled = previous_; }

StackTrace StackTrace::capture() noexcept {
    StackTrace result;
    result.enabled = capture_enabled;
    if (!capture_enabled) return result;
#if defined(_WIN32)
    result.size = CaptureStackBackTrace(1, static_cast<DWORD>(result.frames.size()),
                                       result.frames.data(), nullptr);
#elif defined(KOTAVEL_HAS_BACKTRACE)
    const auto count = ::backtrace(result.frames.data(), static_cast<int>(result.frames.size()));
    if (count > 0) result.size = static_cast<std::size_t>(count);
#endif
    return result;
}

std::string StackTrace::format() const {
    const auto count = std::min(size, frames.size());
    if (!enabled) return "Traza desactivada";
    if (!count) return "Traza no disponible";
    std::ostringstream out;
#if defined(KOTAVEL_HAS_BACKTRACE)
    // backtrace_symbols entrega memoria de malloc; liberarla incluso al lanzar.
    std::unique_ptr<char*, decltype(&std::free)> symbols(
        ::backtrace_symbols(frames.data(), static_cast<int>(count)), &std::free);
#endif
    for (std::size_t i = 0; i < count; ++i) {
        out << '#' << i << ' ';
#if defined(KOTAVEL_HAS_BACKTRACE)
        if (symbols) out << detail::console_safe(std::string_view(symbols.get()[i]).substr(0, 256));
        else
#endif
            out << frames[i];
        out << '\n';
    }
    return out.str();
}


}
