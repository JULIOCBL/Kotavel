#pragma once
#include <kotavel/config/config.hpp>
#include <iosfwd>
#include <memory>

namespace kotavel {
// Ámbito prestado local al hilo. Config debe vivir más que el ámbito y sus vistas.
class ConfigScope final {
public:
    explicit ConfigScope(Config& settings) noexcept;
    ~ConfigScope();
    ConfigScope(const ConfigScope&) = delete;
    ConfigScope& operator=(const ConfigScope&) = delete;
private:
    Config* previous_;
};
Config& config();
// Extraer valores tipados sin copias, como con la instancia explícita.
template<class T> decltype(auto) config(std::string_view key) {
    if constexpr (std::is_same_v<T, ConfigMap>) {
        auto group = config().group(key);
        if (!group) throw std::out_of_range("Configuration group not found: " + std::string(key));
        return ConfigMap(std::move(*group));
    } else return config().get<T>(key);
}

// Vista prestada imprimible; actualizar la clave invalida su uso.
class ConfigValue final {
public:
    ConfigValue() noexcept = default;
    explicit ConfigValue(const Config::Value& value) noexcept : value_(&value) {}
    [[nodiscard]] bool is_null() const noexcept {
        return !value_ || std::holds_alternative<std::monostate>(*value_);
    }
    // Solo las vistas de grupos reconstruidos poseen una copia.
    static ConfigValue own(Config::Value value);
    template<class T> const T& get() const {
        if (value_) if (const auto value = std::get_if<T>(value_)) return *value;
        throw std::invalid_argument("Configuration view type mismatch");
    }
    friend std::ostream& operator<<(std::ostream& stream, const ConfigValue& value);
private:
    std::shared_ptr<const Config::Value> owned_;
    const Config::Value* value_ = nullptr;
};
ConfigValue config(std::string_view key);
ConfigMap config(std::span<const std::string_view> keys);
void config(std::span<const Config::Update> updates);
inline void config(std::initializer_list<Config::Update> updates) { config().set_many(updates); }
// El default se usa solo ante ausencia. Se devuelve por valor para no prestar
// referencias a defaults temporales; los tipos incorrectos siguen siendo errores.
template<class T> T config(std::string_view key, T fallback) {
    const auto value = config().find(key);
    if (!value) return fallback;
    static_assert(std::is_same_v<T, std::string> || std::is_same_v<T, int> ||
                  std::is_same_v<T, double> || std::is_same_v<T, bool> ||
                  std::is_same_v<T, ConfigList> || std::is_same_v<T, ConfigMap> ||
                  std::is_same_v<T, std::monostate>,
                  "Config supports scalar values, null, ConfigList and ConfigMap");
    if (const auto typed = std::get_if<T>(value)) return *typed;
    // Reusar el resultado de find evita un segundo hash de la misma clave.
    throw std::invalid_argument("Configuration type mismatch for key: " + std::string(key));
}
std::string config(std::string_view key, const char* fallback);
std::string config(std::string_view key, std::string_view fallback);
}
