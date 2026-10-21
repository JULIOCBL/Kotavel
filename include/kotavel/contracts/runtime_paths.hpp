#pragma once

#include <filesystem>

namespace kotavel::contracts {

// Ubicaciones de recursos que acompañan al ejecutable o a su bundle.
struct RuntimePaths {
    std::filesystem::path application_root;
    std::filesystem::path plugins;
    std::filesystem::path translations;
    std::filesystem::path certificates;
};

class RuntimePathResolver {
public:
    virtual ~RuntimePathResolver() = default;
    [[nodiscard]] virtual RuntimePaths
    resolve(const std::filesystem::path& executable) const = 0;
};

} // namespace kotavel::contracts
