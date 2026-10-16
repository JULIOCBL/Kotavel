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
    static constexpr OSType type =
#if defined(_WIN32)
        OSType::WINDOWS;
#elif defined(__APPLE__) && defined(__MACH__)
        OSType::MACOS;
#elif defined(__linux__)
        OSType::LINUX;
#else
        OSType::UNKNOWN;
#endif

    [[nodiscard]] static constexpr std::string_view name() noexcept {
        switch (type) {
            case OSType::WINDOWS: return "Windows";
            case OSType::MACOS: return "macOS";
            case OSType::LINUX: return "Linux";
            default: return "Unknown";
        }
    }

    // Identificador estable para claves, catálogos y configuración.
    [[nodiscard]] static constexpr std::string_view key() noexcept {
        switch (type) {
            case OSType::WINDOWS: return "windows";
            case OSType::MACOS: return "macos";
            case OSType::LINUX: return "linux";
            default: return "unknown";
        }
    }

    [[nodiscard]] static constexpr bool isWindows() noexcept {
        return type == OSType::WINDOWS;
    }

    [[nodiscard]] static constexpr bool isMac() noexcept {
        return type == OSType::MACOS;
    }

    [[nodiscard]] static constexpr bool isLinux() noexcept {
        return type == OSType::LINUX;
    }
};

} // namespace kotavel
