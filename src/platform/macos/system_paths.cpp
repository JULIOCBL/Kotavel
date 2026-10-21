#include "../system_paths_common.hpp"

namespace kotavel::platform::detail {
namespace {
class MacOSSystemPaths final : public contracts::SystemPaths {
public:
    std::filesystem::path config_directory(std::string_view application) const override {
        return application_subdirectory(home() / "Library" / "Preferences", application);
    }
    std::filesystem::path data_directory(std::string_view application) const override {
        return application_subdirectory(home() / "Library" / "Application Support", application);
    }
    std::filesystem::path cache_directory(std::string_view application) const override {
        return application_subdirectory(home() / "Library" / "Caches", application);
    }
private:
    static std::filesystem::path home() {
        auto path = environment_path("HOME");
        if (path.empty()) throw std::runtime_error("HOME is unavailable");
        return path;
    }
};
}

std::unique_ptr<contracts::SystemPaths> make_system_paths_provider() {
    return std::make_unique<MacOSSystemPaths>();
}
} // namespace kotavel::platform::detail
