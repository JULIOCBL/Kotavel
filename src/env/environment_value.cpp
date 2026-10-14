#include <kotavel/env/env.hpp>
#include <ostream>
#include <stdexcept>
#include <utility>

namespace kotavel {
// Representación pública de valores: tipos, ausencia y salida a un stream.
EnvironmentValue::EnvironmentValue(EnvValue value) : value_(std::move(value)) {}
bool EnvironmentValue::is_null() const noexcept {
    return std::holds_alternative<std::monostate>(value_);
}
const std::string& EnvironmentValue::as_string() const {
    if (const auto text = std::get_if<std::string>(&value_)) return *text;
    throw std::invalid_argument("Environment value is not a string");
}
bool EnvironmentValue::as_bool() const {
    if (const auto boolean = std::get_if<bool>(&value_)) return *boolean;
    throw std::invalid_argument("Environment value is not a boolean");
}
std::ostream& operator<<(std::ostream& output, const EnvironmentValue& value) {
    // Ausencia/null imprime null; los booleanos siempre usan true/false,
    // independientemente de boolalpha. No crear cadenas auxiliares al imprimir.
    if (const auto text = std::get_if<std::string>(&value.value_)) return output << *text;
    if (const auto boolean = std::get_if<bool>(&value.value_)) return output << (*boolean ? "true" : "false");
    return output << "null";
}

} // namespace kotavel
