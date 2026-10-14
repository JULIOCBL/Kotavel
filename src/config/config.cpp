#include <kotavel/config/config.hpp>
#include <utility>

namespace kotavel {
namespace {
void validate_key(std::string_view key) {
    // Las claves describen opciones, nunca deben contener secretos o controles.
    if (key.empty()) throw std::invalid_argument("Configuration key cannot be empty");
    for (unsigned char c : key)
        if (c < 32 || c == 127) throw std::invalid_argument("Configuration key contains control characters");
}
}
void Config::set(std::string_view key, Value value) {
    validate_key(key);
    if (const auto map = std::get_if<ConfigMap>(&value)) {
        // Sustituir un grupo completo también actualiza sus hojas. Preparar la
        // sustitución antes de publicarla, sin referencias a árboles duplicados.
        Config pending;
        if (!map->empty()) pending.merge("group", *map);
        Config staged = *this;
        const auto prefix = std::string(key) + '.';
        for (auto it = staged.values_.begin(); it != staged.values_.end();) {
            if (it->first == key || std::string_view(it->first).starts_with(prefix))
                it = staged.values_.erase(it);
            else ++it;
        }
        if (map->empty()) staged.values_.emplace(std::string(key), ConfigMap{});
        for (auto& [name, leaf] : pending.values_)
            staged.values_.emplace(prefix + name.substr(6), std::move(leaf));
        values_.swap(staged.values_);
        return;
    }
    // Actualizar no necesita volver a reservar una cadena para la clave existente.
    const auto found = values_.find(key);
    if (found != values_.end()) found->second = std::move(value);
    else values_.emplace(std::string(key), std::move(value));
}
void Config::set(std::string_view key, std::nullptr_t) {
    set(key, Value(std::monostate{}));
}
void Config::set(std::string_view key, int value) {
    set(key, Value(value));
}
void Config::set(std::string_view key, double value) {
    set(key, Value(value));
}
void Config::set(std::string_view key, bool value) {
    set(key, Value(value));
}
void Config::set(std::string_view key, const char* value) {
    // Los literales se almacenan como texto, nunca se convierten a bool.
    if (!value) throw std::invalid_argument("Configuration string cannot be null");
    set(key, Value(std::string(value)));
}
void Config::set(std::string_view key, std::string value) {
    set(key, Value(std::move(value)));
}
void Config::set(std::string_view key, std::string_view value) {
    set(key, Value(std::string(value)));
}
bool Config::contains(std::string_view key) const { return values_.find(key) != values_.end(); }
const Config::Value* Config::find(std::string_view key) const & {
    const auto found = values_.find(key);
    return found == values_.end() ? nullptr : &found->second;
}
const Config::Value& Config::find_value(std::string_view key) const {
    const auto found = values_.find(key);
    if (found == values_.end())
        throw std::out_of_range("Configuration key not found: " + std::string(key));
    return found->second;
}
}
