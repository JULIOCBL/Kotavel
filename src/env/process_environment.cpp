#include <kotavel/env/env.hpp>

#include "environment_internal.hpp"

#include <cstdlib>
#include <cstring>

namespace kotavel {
// Adaptador del sistema: entrega una copia acotada del valor del proceso.
std::optional<std::string> ProcessEnvironment::read(std::string_view key) const {
    detail::validate_key(key);
    // La validación garantiza espacio para la clave y su terminador, incluso
    // cuando string_view es un fragmento de otro buffer sin terminación propia.
    char buffer[257];
    std::memcpy(buffer, key.data(), key.size());
    buffer[key.size()] = '\0';
    const char* value = std::getenv(buffer);
    if (!value) return std::nullopt;
    // Acotar la copia sin escanear valores arbitrariamente grandes.
    std::size_t size = 0;
    while (size <= 4096 && value[size] != '\0') ++size;
    if (size > 4096) throw std::invalid_argument("Environment value exceeds 4096 bytes");
    return std::string(value, size);
}

} // namespace kotavel
