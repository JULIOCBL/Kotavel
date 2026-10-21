#pragma once

#include <string_view>

namespace kotavel {

enum class OSType {
    WINDOWS,
    MACOS,
    LINUX,
    UNKNOWN,
};

class OS final {
public:
    // Definidos por el adaptador de plataforma seleccionado por CMake.
    static const OSType type;

    // Identificador estable para claves, catálogos y configuración.
    [[nodiscard]] static std::string_view name() noexcept;
    [[nodiscard]] static std::string_view key() noexcept;
    [[nodiscard]] static bool isWindows() noexcept;
    [[nodiscard]] static bool isMac() noexcept;
    [[nodiscard]] static bool isLinux() noexcept;
};

} // namespace kotavel
