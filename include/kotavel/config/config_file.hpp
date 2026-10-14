#pragma once
#include <kotavel/config/config_map.hpp>
#include <span>

namespace kotavel {
struct ConfigFile {
    // Prefijo explícito del grupo; no representa una ruta de archivo.
    std::string_view group;
    ConfigMap (*load)();
};
// Cargar en el orden explícito del consumidor y usar directamente el prefijo declarado.
Config load_config_files(std::span<const ConfigFile> files);
}
