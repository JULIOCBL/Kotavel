#include <kotavel/config/config_file.hpp>

namespace kotavel {
Config load_config_files(std::span<const ConfigFile> files) {
    Config result;
    for (const auto& file : files) {
        if (!file.load) throw std::invalid_argument("Configuration file requires a loader");
        const auto group = file.group;
        if (group.empty() || group.find_first_not_of("abcdefghijklmnopqrstuvwxyz0123456789_") != group.npos ||
            group.front() < 'a' || group.front() > 'z')
            throw std::invalid_argument("Configuration group requires a lowercase identifier");
        // El registro define el orden; merge conserva las consultas planas y detecta colisiones.
        result.merge(group, file.load());
    }
    return result;
}
}
