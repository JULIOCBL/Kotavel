#pragma once

#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

namespace kotavel::detail {

class ConsoleSettingsError final : public std::invalid_argument {
public:
    ConsoleSettingsError(std::string key, std::string reason)
        : std::invalid_argument("Invalid configuration value for " + key + ": " + reason),
          key_(std::move(key)), reason_(std::move(reason)) {}

    [[nodiscard]] std::string_view key() const noexcept { return key_; }
    [[nodiscard]] std::string_view reason() const noexcept { return reason_; }

private:
    std::string key_;
    std::string reason_;
};

} // namespace kotavel::detail
