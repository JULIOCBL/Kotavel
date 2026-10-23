#pragma once

#include <cstdint>
#include <filesystem>
#include <optional>

namespace kotavel::metrics::detail {

using ProcessIsRunning = bool (*)(std::uint64_t) noexcept;

[[nodiscard]] std::optional<std::uint64_t>
endpoint_process_id(const std::filesystem::path& path);

void remove_stale_endpoints(const std::filesystem::path& directory,
                            std::uint64_t current_process_id,
                            ProcessIsRunning process_is_running) noexcept;

} // namespace kotavel::metrics::detail
