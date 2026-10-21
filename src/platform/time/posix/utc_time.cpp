#include "../../utc_time.hpp"

namespace kotavel::platform::detail {
bool to_utc_time(std::time_t value, std::tm& result) noexcept {
    return gmtime_r(&value, &result) != nullptr;
}
} // namespace kotavel::platform::detail
