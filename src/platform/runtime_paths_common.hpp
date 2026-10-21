#pragma once

#include <kotavel/contracts/runtime_paths.hpp>

#include <filesystem>
#include <stdexcept>

namespace kotavel::platform::detail {

inline std::filesystem::path absolute_executable(const std::filesystem::path& executable) {
    if (executable.empty()) throw std::invalid_argument("Executable path cannot be empty");
    std::error_code error;
    auto absolute = std::filesystem::absolute(executable, error);
    if (error) throw std::filesystem::filesystem_error("Cannot resolve executable path", executable, error);
    return absolute.lexically_normal();
}

inline contracts::RuntimePaths sibling_runtime_paths(const std::filesystem::path& executable) {
    const auto root = absolute_executable(executable).parent_path();
    return {root, root / "plugins", root / "translations", root / "certificates"};
}

} // namespace kotavel::platform::detail
