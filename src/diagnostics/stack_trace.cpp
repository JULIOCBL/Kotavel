#include <kotavel/diagnostics/error.hpp>
#include "../platform/stack_trace.hpp"

#include <algorithm>

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
    result.size = platform::detail::capture_stack_trace(result.frames);
    return result;
}

std::string StackTrace::format() const {
    const auto count = std::min(size, frames.size());
    if (!enabled) return "Traza desactivada";
    if (!count) return "Traza no disponible";
    return platform::detail::format_stack_trace(
        std::span<void* const>(frames.data(), count));
}


}
