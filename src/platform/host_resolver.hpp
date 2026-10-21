#pragma once

#include <string_view>

namespace kotavel::platform::detail {

[[nodiscard]] bool resolves_host(std::string_view host);

} // namespace kotavel::platform::detail
