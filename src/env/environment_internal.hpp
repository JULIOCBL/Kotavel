#pragma once
#include <kotavel/env/env.hpp>
#include <stdexcept>

namespace kotavel::detail {
// Validación compartida; detalle privado, no forma parte de la API instalada.
inline void validate_key(std::string_view key) {
    if (key.empty() || key.size() > 256 || key.find_first_of("=\0", 0, 2) != key.npos)
        throw std::invalid_argument("Invalid environment key");
}

// Lector activo del hilo o lector predeterminado del proceso.
const Env& current_environment();
}
