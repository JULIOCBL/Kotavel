#pragma once

#include <charconv>
#include <cmath>
#include <cstdint>
#include <iterator>
#include <limits>
#include <map>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <unordered_map>
#include <utility>
#include <variant>
#include <vector>

#include <kotavel/http/router.hpp>

namespace kotavel::http {

// Representación JSON pública de Kotavel. Su contrato no depende de una biblioteca concreta.
class JsonValue final {
public:
    using Array = std::vector<JsonValue>;
    using Object = std::map<std::string, JsonValue>;
    using Storage = std::variant<std::nullptr_t, bool, std::int64_t, std::uint64_t,
                                 double, std::string, Array, Object>;

    JsonValue() : value_(nullptr) {}
    JsonValue(std::nullptr_t) : value_(nullptr) {}
    JsonValue(bool value) : value_(value) {}

    template <typename T, std::enable_if_t<std::is_integral_v<T> &&
              std::is_signed_v<T> && !std::is_same_v<T, bool>, int> = 0>
    JsonValue(T value) : value_(static_cast<std::int64_t>(value)) {}

    template <typename T, std::enable_if_t<std::is_integral_v<T> &&
              std::is_unsigned_v<T>, int> = 0>
    JsonValue(T value) : value_(static_cast<std::uint64_t>(value)) {}

    template <typename T, std::enable_if_t<std::is_floating_point_v<T>, int> = 0>
    JsonValue(T value) : value_(static_cast<double>(value)) {
        if (!std::isfinite(static_cast<double>(value)))
            throw std::invalid_argument("JSON numbers must be finite");
    }

    JsonValue(std::string value) : value_(std::move(value)) {}
    JsonValue(std::string_view value) : value_(std::string(value)) {}
    JsonValue(const char* value) : value_(value == nullptr ? Storage(nullptr) : Storage(std::string(value))) {}
    JsonValue(Array value) : value_(std::move(value)) {}
    JsonValue(Object value) : value_(std::move(value)) {}

    friend bool operator==(const JsonValue&, const JsonValue&) = default;

    [[nodiscard]] const Storage& value() const noexcept { return value_; }

private:
    Storage value_;
};

// Especializa este tipo para convertir tus structs a tipos JSON de Kotavel.
// La especialización debe implementar: static JsonValue serialize(const T&).
template <typename T, typename Enable = void>
struct ApiJsonSerializer {
    static JsonValue serialize(const T&) {
        static_assert(sizeof(T) == 0,
                      "ApiResponse cannot serialize this type; specialize ApiJsonSerializer<T>");
        return {};
    }
};

namespace detail {

template <typename T>
JsonValue to_json(const T& value) {
    return ApiJsonSerializer<std::remove_cvref_t<T>>::serialize(value);
}

template <std::size_t N>
JsonValue to_json(const char (&value)[N]) {
    return JsonValue(std::string_view(value, N - 1));
}

template <typename K>
std::string json_object_key(const K& key) {
    if constexpr (std::is_convertible_v<const K&, std::string_view>) {
        return std::string(std::string_view(key));
    } else if constexpr (std::is_same_v<K, bool>) {
        return key ? "true" : "false";
    } else if constexpr (std::is_integral_v<K>) {
        char buffer[std::numeric_limits<K>::digits10 + 4]{};
        const auto [end, error] = std::to_chars(std::begin(buffer), std::end(buffer), key);
        if (error != std::errc{}) throw std::invalid_argument("Could not serialize JSON object key");
        return std::string(buffer, end);
    } else {
        static_assert(std::is_same_v<K, void>,
                      "JSON map keys must be strings or integral values");
    }
}

template <typename Map>
JsonValue map_to_json(const Map& values) {
    JsonValue::Object output;
    for (const auto& [key, value] : values)
        output.emplace(json_object_key(key), to_json(value));
    return JsonValue(std::move(output));
}

} // namespace detail

template <>
struct ApiJsonSerializer<JsonValue> {
    static JsonValue serialize(const JsonValue& value) { return value; }
};

template <>
struct ApiJsonSerializer<std::nullptr_t> {
    static JsonValue serialize(std::nullptr_t) { return nullptr; }
};

template <>
struct ApiJsonSerializer<bool> {
    static JsonValue serialize(bool value) { return value; }
};

template <typename T>
struct ApiJsonSerializer<T, std::enable_if_t<std::is_integral_v<T> &&
                                              !std::is_same_v<T, bool>>> {
    static JsonValue serialize(T value) { return JsonValue(value); }
};

template <typename T>
struct ApiJsonSerializer<T, std::enable_if_t<std::is_floating_point_v<T>>> {
    static JsonValue serialize(T value) { return JsonValue(value); }
};

template <>
struct ApiJsonSerializer<std::string> {
    static JsonValue serialize(const std::string& value) { return JsonValue(value); }
};

template <>
struct ApiJsonSerializer<std::string_view> {
    static JsonValue serialize(std::string_view value) { return JsonValue(value); }
};

template <>
struct ApiJsonSerializer<const char*> {
    static JsonValue serialize(const char* value) { return JsonValue(value); }
};

template <typename T, typename Allocator>
struct ApiJsonSerializer<std::vector<T, Allocator>> {
    static JsonValue serialize(const std::vector<T, Allocator>& values) {
        JsonValue::Array output;
        output.reserve(values.size());
        for (const auto& value : values) {
            if constexpr (std::is_same_v<T, bool>) output.emplace_back(static_cast<bool>(value));
            else output.push_back(detail::to_json(value));
        }
        return JsonValue(std::move(output));
    }
};

template <typename K, typename V, typename Compare, typename Allocator>
struct ApiJsonSerializer<std::map<K, V, Compare, Allocator>> {
    static JsonValue serialize(const std::map<K, V, Compare, Allocator>& values) {
        return detail::map_to_json(values);
    }
};

template <typename K, typename V, typename Hash, typename Equal, typename Allocator>
struct ApiJsonSerializer<std::unordered_map<K, V, Hash, Equal, Allocator>> {
    static JsonValue serialize(const std::unordered_map<K, V, Hash, Equal, Allocator>& values) {
        return detail::map_to_json(values);
    }
};

struct ApiError final {
    std::string title;
    std::string detail;
    std::optional<std::string> field;
};

// Formato público de respuestas JSON para handlers API y errores del Kernel.
class ApiResponse final {
public:
    template <typename T>
    [[nodiscard]] static Response success(const T& data, int status = 200,
                                          std::string_view title = {},
                                          std::string_view message = {}) {
        return success_json(detail::to_json(data), status, title, message);
    }

    [[nodiscard]] static Response error(int status, const std::vector<ApiError>& errors);
    [[nodiscard]] static Response error(int status, std::string_view title,
                                        std::string_view message);

private:
    [[nodiscard]] static Response success_json(JsonValue data, int status,
                                               std::string_view title,
                                               std::string_view message);
};

} // namespace kotavel::http
