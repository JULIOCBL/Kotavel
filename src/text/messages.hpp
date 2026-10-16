#pragma once

#include <initializer_list>
#include <string>
#include <string_view>
#include <utility>

namespace kotavel {

// Catálogos internos incorporados al núcleo durante la compilación.
class Messages final {
public:
    using Parameter = std::pair<std::string_view, std::string_view>;

    // Clave: catálogo.ruta.del.texto. Los parámetros se pasan sin ':'.
    // Lanza invalid_argument ante claves/nombres mal formados, valores ausentes,
    // duplicados o sobrantes; out_of_range si la clave válida no existe.
    [[nodiscard]] static std::string get(
        std::string_view key, std::initializer_list<Parameter> parameters = {});
};

} // namespace kotavel
