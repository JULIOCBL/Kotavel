#pragma once

#include <cstdint>
#include <source_location>

namespace kotavel {

#if defined(__apple_build_version__) && __clang_major__ < 16
// Apple Clang 15 con SDK recientes puede evaluar source_location::current()
// en la declaración del argumento por defecto. Los builtins conservan el
// lugar de llamada; la misma prueba de archivo/línea valida ambas variantes.
class SourceLocation {
public:
    static constexpr SourceLocation current(
        const char* file = __builtin_FILE(),
        std::uint_least32_t line = __builtin_LINE(),
        const char* function = __builtin_FUNCTION()) noexcept {
        SourceLocation value;
        value.file_ = file;
        value.line_ = line;
        value.function_ = function;
        return value;
    }
    constexpr const char* file_name() const noexcept { return file_; }
    constexpr const char* function_name() const noexcept { return function_; }
    constexpr std::uint_least32_t line() const noexcept { return line_; }
private:
    const char* file_ = "";
    const char* function_ = "";
    std::uint_least32_t line_ = 0;
};
#else
using SourceLocation = std::source_location;
#endif

} // namespace kotavel
