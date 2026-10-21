#include "../../system_paths_common.hpp"

namespace kotavel::platform::detail {
namespace {
class LinuxSystemPaths final : public contracts::SystemPaths {
public:
    std::filesystem::path config_directory(std::string_view application) const override {
        return application_subdirectory(xdg_root("XDG_CONFIG_HOME", ".config"), application);
    }
    std::filesystem::path data_directory(std::string_view application) const override {
        return application_subdirectory(xdg_root("XDG_DATA_HOME", ".local/share"), application);
    }
    std::filesystem::path cache_directory(std::string_view application) const override {
        return application_subdirectory(xdg_root("XDG_CACHE_HOME", ".cache"), application);
    }
private:
    static std::filesystem::path xdg_root(const char* variable, const char* fallback) {
        auto path = environment_path(variable);
        // XDG_*_HOME debe ser absoluto; los valores relativos se ignoran.
        if (!path.empty() && path.is_absolute()) return path;
        const auto home = environment_path("HOME");
        if (home.empty()) throw std::runtime_error("HOME is unavailable");
        return home / fallback;
    }
};
}

std::unique_ptr<contracts::SystemPaths> make_system_paths_provider() {
    return std::make_unique<LinuxSystemPaths>();
}
} // namespace kotavel::platform::detail
