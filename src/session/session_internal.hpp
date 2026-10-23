#pragma once

#include <span>
#include <string_view>

namespace kotavel::http::detail {

void fill_secure_random(std::span<unsigned char> bytes);
[[nodiscard]] bool valid_session_id(std::string_view id) noexcept;
[[nodiscard]] std::string generate_session_id();

} // namespace kotavel::http::detail
