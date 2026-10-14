#pragma once
#include <string>
#include <string_view>

namespace kotavel::detail {
// Evitar caracteres de control en símbolos y mensajes mostrados en consola.
inline std::string console_safe(std::string_view value) {
    std::string result;
    for (unsigned char c : value) {
        if (c < 32 || c == 127) result += ' ';
        else result += static_cast<char>(c);
    }
    return result;
}
}
