#include "../../../src/text/messages.hpp"

#include <stdexcept>

int main() {
    using kotavel::Messages;
    if (Messages::get("env.open_failed.windows.description", {{"file", ".env"}}) !=
        "No se pudo abrir el archivo de entorno `.env` en la raíz del proyecto.") return 1;
    if (Messages::get("env.open_failed.windows.command", {{"file", ".env"}, {"example", ".env.example"}}) !=
        "En PowerShell: Copy-Item -LiteralPath '.env.example' -Destination '.env'") return 1;
    if (Messages::get("env.open_failed.macos.command", {{"example", ".env.example"}, {"file", ".env"}}) !=
        "En Terminal: cp '.env.example' '.env'") return 1;
    if (Messages::get("env.open_failed.linux.command", {{"example", ".env.example"}, {"file", ".env"}}) !=
        "En la terminal: cp '.env.example' '.env'") return 1;
    if (Messages::get("env.open_failed.unknown.command", {{"example", ".env.example"}, {"file", ".env"}}) !=
        "Copia manualmente `.env.example` como `.env`.") return 1;
    // Un valor insertado conserva sus dos puntos, Unicode y marcadores literalmente.
    if (Messages::get("env.open_failed.windows.action", {{"file", ".env"}, {"example", "C:\\área\\:file.example"}}) !=
        "Verifica que `.env` exista y tenga permisos de lectura. Si falta, créalo desde `C:\\área\\:file.example`.") return 1;
    try {
        (void)Messages::get("env.unknown");
        return 1;
    } catch (const std::out_of_range&) {}
    try {
        (void)Messages::get("env.open_failed.windows.description", {{"filename", ".env"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    try {
        (void)Messages::get("env.open_failed.windows.description", {{"file", "first"}, {"file", "second"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    try {
        (void)Messages::get("env.open_failed.windows.description", {{"fiel", ".env"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    try {
        (void)Messages::get("env.open_failed.windows.description", {{"unused", ".env"}, {"file", ".env"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    try {
        (void)Messages::get("env..open_failed.windows.description", {{"file", ".env"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    try {
        (void)Messages::get("env.open_failed.windows.description", {{"bad-name", ".env"}});
        return 1;
    } catch (const std::invalid_argument&) {}
    return 0;
}
