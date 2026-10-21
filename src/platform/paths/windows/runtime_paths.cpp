#include "../../runtime_paths_common.hpp"

namespace kotavel::platform::detail {
std::unique_ptr<contracts::RuntimePathResolver> make_runtime_path_resolver() {
    class WindowsRuntimePathResolver final : public contracts::RuntimePathResolver {
    public:
        contracts::RuntimePaths resolve(const std::filesystem::path& executable) const override {
            return sibling_runtime_paths(executable);
        }
    };
    return std::make_unique<WindowsRuntimePathResolver>();
}
} // namespace kotavel::platform::detail
