#pragma once

#include <span>
#include <string>

namespace kotavel::platform::detail {

[[nodiscard]] std::size_t capture_stack_trace(std::span<void*> frames) noexcept;
[[nodiscard]] std::string format_stack_trace(std::span<void* const> frames);

} // namespace kotavel::platform::detail
