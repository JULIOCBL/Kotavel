#include "../application_bundle.hpp"

namespace kotavel::platform::detail {
std::optional<InstalledBundle> installed_bundle(const char*) { return std::nullopt; }
} // namespace kotavel::platform::detail
