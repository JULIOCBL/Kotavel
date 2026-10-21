#pragma once

#include <kotavel/contracts/system_paths.hpp>

#include <cstdlib>
#include <string_view>
#include <stdexcept>
#include <string>

namespace kotavel::platform::detail {

inline std::filesystem::path environment_path(const char* name) {
    const char* value = std::getenv(name);
    return value && *value ? std::filesystem::path(value) : std::filesystem::path{};
}

inline std::filesystem::path application_subdirectory(
    std::filesystem::path base, std::string_view application) {
    if (application.empty() || application == "." || application == ".." ||
        application.find_first_of("/\\") != std::string_view::npos)
        throw std::invalid_argument("Application path name must be a single non-empty component");
    return base / std::string(application);
}

} // namespace kotavel::platform::detail
