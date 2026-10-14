#include <kotavel/env/env.hpp>
#include "environment_internal.hpp"
#include <utility>

namespace kotavel {
// Conversión y acceso tipado; la lectura concreta corresponde a EnvironmentSource.
namespace {
bool ascii_equals(std::string_view value, std::string_view expected) noexcept {
    if (value.size() != expected.size()) return false;
    for (std::size_t i = 0; i < value.size(); ++i) {
        auto c = static_cast<unsigned char>(value[i]);
        if (c >= 'A' && c <= 'Z') c += 'a' - 'A';
        if (c != static_cast<unsigned char>(expected[i])) return false;
    }
    return true;
}
EnvValue decode(std::string value) {
    if (value.size() > 4096) throw std::invalid_argument("Environment value exceeds 4096 bytes");
    // Comparar sin copiar ni depender de la configuración regional del proceso.
    if (ascii_equals(value, "true") || ascii_equals(value, "(true)")) return true;
    if (ascii_equals(value, "false") || ascii_equals(value, "(false)")) return false;
    if (ascii_equals(value, "null") || ascii_equals(value, "(null)")) return std::monostate{};
    if (ascii_equals(value, "empty") || ascii_equals(value, "(empty)")) return std::string{};
    if (value.size() >= 2 && (value.front() == '\'' || value.front() == '"') && value.back() == value.front())
    {
        value.pop_back();
        value.erase(0, 1);
    }
    return value;
}
}
EnvValue Env::get(std::string_view key, EnvValue fallback) const {
    detail::validate_key(key);
    auto value = source_.read(key);
    return value ? decode(std::move(*value)) : std::move(fallback);
}
EnvValue Env::require(std::string_view key) const {
    auto value = get(key);
    if (std::holds_alternative<std::monostate>(value))
        throw std::invalid_argument("Required environment variable is missing: " + std::string(key));
    return value;
}
std::string Env::string(std::string_view key, std::string fallback) const {
    auto value = get(key, std::move(fallback));
    if (auto result = std::get_if<std::string>(&value)) return std::move(*result);
    throw std::invalid_argument("Environment variable must be a string: " + std::string(key));
}
bool Env::boolean(std::string_view key, bool fallback) const {
    auto value = get(key, fallback);
    if (auto result = std::get_if<bool>(&value)) return *result;
    throw std::invalid_argument("Environment variable must be true or false: " + std::string(key));
}

} // namespace kotavel
