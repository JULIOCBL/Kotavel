#include "../../utc_time.hpp"

namespace kotavel::platform::detail {
bool to_utc_time(std::time_t value, std::tm& result) noexcept {
    return gmtime_s(&result, &value) == 0;
}
} // namespace kotavel::platform::detail
