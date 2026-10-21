#include "../runtime_paths_common.hpp"

namespace kotavel::platform::detail {
namespace {
class MacOSRuntimePathResolver final : public contracts::RuntimePathResolver {
public:
    contracts::RuntimePaths resolve(const std::filesystem::path& executable) const override {
        const auto binary = absolute_executable(executable);
        const auto macos = binary.parent_path();
        const auto contents = macos.parent_path();
        const auto bundle = contents.parent_path();
        if (macos.filename() == "MacOS" && contents.filename() == "Contents" &&
            bundle.extension() == ".app") {
            const auto resources = contents / "Resources";
            return {bundle, contents / "PlugIns", resources / "translations",
                    resources / "certificates"};
        }
        return sibling_runtime_paths(binary);
    }
};
}

std::unique_ptr<contracts::RuntimePathResolver> make_runtime_path_resolver() {
    return std::make_unique<MacOSRuntimePathResolver>();
}
} // namespace kotavel::platform::detail
