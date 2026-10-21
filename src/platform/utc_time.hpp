#pragma once

#include <ctime>

namespace kotavel::platform::detail {

[[nodiscard]] bool to_utc_time(std::time_t value, std::tm& result) noexcept;

} // namespace kotavel::platform::detail
