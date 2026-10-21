#include "../system_paths_common.hpp"

namespace kotavel::platform::detail {
namespace {
class WindowsSystemPaths final : public contracts::SystemPaths {
public:
    std::filesystem::path config_directory(std::string_view application) const override {
        return application_subdirectory(roaming_root(), application);
    }
    std::filesystem::path data_directory(std::string_view application) const override {
        return application_subdirectory(roaming_root(), application);
    }
    std::filesystem::path cache_directory(std::string_view application) const override {
        return application_subdirectory(local_root(), application) / "Cache";
    }
private:
    static std::filesystem::path home() {
        auto path = environment_path("USERPROFILE");
        if (path.empty()) throw std::runtime_error("USERPROFILE is unavailable");
        return path;
    }
    static std::filesystem::path roaming_root() {
        auto path = environment_path("APPDATA");
        return path.empty() ? home() / "AppData" / "Roaming" : path;
    }
    static std::filesystem::path local_root() {
        auto path = environment_path("LOCALAPPDATA");
        return path.empty() ? home() / "AppData" / "Local" : path;
    }
};
}

std::unique_ptr<contracts::SystemPaths> make_system_paths_provider() {
    return std::make_unique<WindowsSystemPaths>();
}
} // namespace kotavel::platform::detail
