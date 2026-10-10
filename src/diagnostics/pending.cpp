#include <kotavel/diagnostics/pending.hpp>

namespace kotavel {
FeatureStatus configure_remote_logging(std::string_view) noexcept {
    return FeatureStatus::not_implemented;
}
FeatureStatus enable_fatal_system_capture() noexcept {
    return FeatureStatus::not_implemented;
}
} // namespace kotavel
