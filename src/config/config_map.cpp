#include <kotavel/config/config_map.hpp>
#include <vector>

namespace kotavel {
namespace {
void flatten(std::string_view prefix, const ConfigMap& map, Config& pending, unsigned depth,
             std::size_t& count) {
    if (depth > 32) throw std::invalid_argument("Configuration nesting exceeds 32 levels");
    for (const auto& [key, node] : map) {
        if (++count > 10000) throw std::invalid_argument("Configuration map exceeds 10000 nodes");
        if (key.empty() || key.front() == '.' || key.back() == '.' || key.find("..") != key.npos)
            throw std::invalid_argument("Invalid configuration map key");
        // Reservar y ensamblar una vez evita temporales al aplanar cada entrada.
        std::string full;
        full.reserve(prefix.size() + 1 + key.size());
        full.append(prefix);
        full.push_back('.');
        full.append(key);
        if (full.size() > 1024) throw std::invalid_argument("Configuration key exceeds 1024 bytes");
        if (const auto group = std::get_if<ConfigMap>(&node.value)) {
            if (group->empty()) pending.set(full, ConfigMap{});
            else flatten(full, *group, pending, depth + 1, count);
        } else {
            if (pending.contains(full)) throw std::invalid_argument("Duplicate flattened configuration key: " + full);
            pending.set(full, node.value);
        }
    }
}
}
void Config::merge(std::string_view group, const ConfigMap& map) {
    if (group.empty() || group.find('.') != group.npos)
        throw std::invalid_argument("Configuration group requires a single name");
    if (map.empty()) {
        // Un grupo registrado vacío también existe y puede recuperarse como mapa.
        if (contains(group) || this->group(group))
            throw std::invalid_argument("Duplicate configuration group: " + std::string(group));
        for (unsigned char c : group)
            if (c < 32 || c == 127) throw std::invalid_argument("Invalid configuration group");
        values_.emplace(std::string(group), Value(ConfigMap{}));
        return;
    }
    Config pending;
    std::size_t count = 0;
    flatten(group, map, pending, 0, count);
    // Comprobar todas las colisiones antes de transferir los nodos al mapa plano.
    for (const auto& [key, value] : pending.values_)
        if (contains(key)) throw std::invalid_argument("Duplicate configuration key: " + key);
    values_.merge(pending.values_);
}
}
