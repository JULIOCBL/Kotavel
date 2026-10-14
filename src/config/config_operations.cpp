#include <kotavel/config/config.hpp>

namespace kotavel {
void Config::set_many(std::span<const Update> updates) {
    if (updates.empty()) return;
    Config staged = *this;
    // La copia se usa solo para escrituras múltiples, nunca al leer una hoja.
    for (const auto& update : updates) staged.set(update.key, update.value);
    values_.swap(staged.values_);
}
ConfigMap Config::get_many(std::span<const std::string_view> keys) const {
    ConfigMap result;
    for (const auto key : keys) {
        if (const auto item = find(key)) result.insert_or_assign(std::string(key), ConfigNode(*item));
        else if (auto map = group(key)) result.insert_or_assign(std::string(key), ConfigNode(std::move(*map)));
        else result.insert_or_assign(std::string(key), ConfigNode(nullptr));
    }
    return result;
}
std::optional<ConfigMap> Config::group(std::string_view prefix) const {
    if (const auto direct = find(prefix)) {
        if (const auto map = std::get_if<ConfigMap>(direct)) return *map;
        return std::nullopt; // Un valor explícito tiene prioridad sobre prefijos.
    }
    const auto start = std::string(prefix) + '.';
    ConfigMap result;
    bool found = false;
    for (const auto& [key, value] : values_) {
        if (!std::string_view(key).starts_with(start)) continue;
        found = true;
        std::string_view remaining(key.data() + start.size(), key.size() - start.size());
        auto* target = &result;
        // Solo reconstruir segmentos cuando se pide una copia del grupo.
        while (true) {
            const auto dot = remaining.find('.');
            const auto part = remaining.substr(0, dot);
            if (dot == remaining.npos) {
                if (target->find(part) != target->end())
                    throw std::invalid_argument("Conflicting configuration group structure");
                target->emplace(std::string(part), ConfigNode(value));
                break;
            }
            // ConfigMap tiene búsqueda transparente: reutilizar segmentos ya
            // creados evita reservar una cadena y un mapa temporal por cada hoja.
            auto node = target->find(part);
            if (node == target->end())
                node = target->emplace(std::string(part), ConfigMap{}).first;
            auto child = std::get_if<ConfigMap>(&node->second.value);
            if (!child) throw std::invalid_argument("Conflicting configuration group structure");
            target = child;
            remaining.remove_prefix(dot + 1);
        }
    }
    return found ? std::optional<ConfigMap>(std::move(result)) : std::nullopt;
}
}
