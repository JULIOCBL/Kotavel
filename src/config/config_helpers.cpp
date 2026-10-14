#include <kotavel/config/config_helpers.hpp>
#include <ostream>

namespace kotavel {
namespace { thread_local Config* active_config = nullptr; }
// El ámbito anidado restaura la instancia anterior, también ante excepciones.
ConfigScope::ConfigScope(Config& settings) noexcept : previous_(active_config) { active_config = &settings; }
ConfigScope::~ConfigScope() { active_config = previous_; }
Config& config() {
    if (!active_config) throw std::logic_error("No active application configuration");
    return *active_config;
}
ConfigValue config(std::string_view key) {
    const auto value = config().find(key);
    if (value) return ConfigValue(*value);
    if (auto group = config().group(key)) return ConfigValue::own(std::move(*group));
    return ConfigValue{};
}
ConfigValue ConfigValue::own(Config::Value value) {
    ConfigValue result;
    result.owned_ = std::make_shared<const Config::Value>(std::move(value));
    result.value_ = result.owned_.get();
    return result;
}
ConfigMap config(std::span<const std::string_view> keys) { return config().get_many(keys); }
void config(std::span<const Config::Update> updates) { config().set_many(updates); }
std::string config(std::string_view key, const char* fallback) {
    if (!fallback) throw std::invalid_argument("Configuration string fallback cannot be null");
    return config(key, std::string_view(fallback));
}
std::string config(std::string_view key, std::string_view fallback) {
    const auto value = config().find(key);
    if (!value) return std::string(fallback);
    if (const auto text = std::get_if<std::string>(value)) return *text;
    throw std::invalid_argument("Configuration type mismatch for key: " + std::string(key));
}
std::ostream& operator<<(std::ostream& stream, const ConfigValue& value) {
    // Ausencia se representa como null; no confundirla con false o texto vacío.
    if (value.is_null()) return stream << "null";
    std::visit([&](const auto& item) {
        using T = std::decay_t<decltype(item)>;
        if constexpr (std::is_same_v<T, bool>) stream << (item ? "true" : "false");
        else if constexpr (std::is_same_v<T, std::monostate>) stream << "null";
        // No volcar grupos o listas completos: pueden contener secretos.
        else if constexpr (std::is_same_v<T, ConfigMap>) stream << "[configuration group]";
        else if constexpr (std::is_same_v<T, ConfigList>) stream << "[configuration list]";
        else stream << item;
    }, *value.value_);
    return stream;
}
}
