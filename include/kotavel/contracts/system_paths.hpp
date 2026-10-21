#pragma once

#include <filesystem>
#include <string_view>

namespace kotavel::contracts {

// Resuelve las carpetas estándar del sistema para datos de una aplicación.
class SystemPaths {
public:
    virtual ~SystemPaths() = default;
    [[nodiscard]] virtual std::filesystem::path
    config_directory(std::string_view application) const = 0;
    [[nodiscard]] virtual std::filesystem::path
    data_directory(std::string_view application) const = 0;
    [[nodiscard]] virtual std::filesystem::path
    cache_directory(std::string_view application) const = 0;
};

} // namespace kotavel::contracts
