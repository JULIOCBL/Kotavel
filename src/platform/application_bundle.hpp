#pragma once

#include <filesystem>
#include <optional>
#include <string>

namespace kotavel::platform::detail {

struct InstalledBundle {
    std::filesystem::path resources;
    std::filesystem::path data;
    std::string environment = "production";
};

[[nodiscard]] std::optional<InstalledBundle> installed_bundle(const char* executable);

} // namespace kotavel::platform::detail
