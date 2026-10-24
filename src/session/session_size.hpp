#pragma once

#include <kotavel/session/session.hpp>

#include <limits>
#include <type_traits>

namespace kotavel::http::detail {

inline void add_saturated(std::size_t& total, std::size_t value) noexcept {
    if (value > std::numeric_limits<std::size_t>::max() - total)
        total = std::numeric_limits<std::size_t>::max();
    else total += value;
}

inline std::size_t estimate_json_value_size(const JsonValue& value) {
    return std::visit([](const auto& item) -> std::size_t {
        using T = std::decay_t<decltype(item)>;
        if constexpr (std::is_same_v<T, std::string>) return sizeof(T) + item.size();
        else if constexpr (std::is_same_v<T, JsonValue::Array>) {
            std::size_t result = sizeof(T);
            if (item.size() > (std::numeric_limits<std::size_t>::max() - result) /
                              sizeof(JsonValue)) return std::numeric_limits<std::size_t>::max();
            result += item.size() * sizeof(JsonValue);
            for (const auto& child : item) add_saturated(result, estimate_json_value_size(child));
            return result;
        } else if constexpr (std::is_same_v<T, JsonValue::Object>) {
            std::size_t result = sizeof(T);
            for (const auto& [key, child] : item) {
                add_saturated(result, sizeof(key) + key.size());
                add_saturated(result, estimate_json_value_size(child));
            }
            return result;
        } else return sizeof(T);
    }, value.value());
}

inline std::size_t estimate_session_values_size(const JsonValue::Object& values) {
    std::size_t result = sizeof(values);
    for (const auto& [key, value] : values) {
        add_saturated(result, sizeof(key) + key.size());
        add_saturated(result, estimate_json_value_size(value));
    }
    return result;
}

inline std::size_t estimate_session_values_replacing(std::size_t current_size,
                                                      std::string_view key,
                                                      const JsonValue* previous,
                                                      const JsonValue& replacement) {
    auto result = current_size;
    if (result == std::numeric_limits<std::size_t>::max()) return result;
    if (previous) {
        const auto old_size = sizeof(std::string) + key.size() +
            estimate_json_value_size(*previous);
        result -= old_size;
    }
    add_saturated(result, sizeof(std::string) + key.size());
    add_saturated(result, estimate_json_value_size(replacement));
    return result;
}

inline std::size_t estimate_session_values_replacing(std::size_t current_size,
                                                      const JsonValue::Object& values,
                                                      std::string_view key,
                                                      const JsonValue& replacement) {
    const auto found = values.find(std::string(key));
    return estimate_session_values_replacing(current_size, key,
        found == values.end() ? nullptr : &found->second, replacement);
}

inline std::size_t estimate_session_values_erasing(std::size_t current_size,
                                                    std::string_view key,
                                                    const JsonValue* previous) {
    if (!previous) return current_size;
    const auto removed = sizeof(std::string) + key.size() + estimate_json_value_size(*previous);
    return removed > current_size ? 0 : current_size - removed;
}

inline std::size_t estimate_session_values_erasing(std::size_t current_size,
                                                    const JsonValue::Object& values,
                                                    std::string_view key) {
    const auto found = values.find(std::string(key));
    return estimate_session_values_erasing(current_size, key,
        found == values.end() ? nullptr : &found->second);
}

inline std::size_t estimate_session_record_size(const contracts::SessionRecord& record) {
    std::size_t result = sizeof(record) + record.id.size();
    add_saturated(result, record.estimated_values_bytes != 0 ? record.estimated_values_bytes :
                  estimate_session_values_size(record.values));
    return result;
}

} // namespace kotavel::http::detail
