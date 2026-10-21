#include "../runtime_paths_common.hpp"

namespace kotavel::platform::detail {
namespace {
class LinuxRuntimePathResolver final : public contracts::RuntimePathResolver {
public:
    contracts::RuntimePaths resolve(const std::filesystem::path& executable) const override {
        const auto binary = absolute_executable(executable);
        const auto bin = binary.parent_path();
        const auto prefix = bin.filename() == "bin" ? bin.parent_path() : bin;
        const auto app = binary.stem().string();
        const auto share = prefix / "share" / app;
        return {prefix, prefix / "lib" / "kotavel" / "plugins",
                share / "translations", share / "certificates"};
    }
};
}

std::unique_ptr<contracts::RuntimePathResolver> make_runtime_path_resolver() {
    return std::make_unique<LinuxRuntimePathResolver>();
}
} // namespace kotavel::platform::detail
