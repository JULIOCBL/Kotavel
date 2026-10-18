#include <kotavel/diagnostics/error.hpp>

#include "console_text.hpp"

namespace kotavel {
// Separar la presentación pública del diagnóstico interno conservado en memoria.
RenderedError render_error(const ErrorDetails& error, Environment environment,
                            std::string_view public_message) {
    if (environment != Environment::production && environment != Environment::development)
        throw std::invalid_argument("Invalid presentation environment");
    RenderedError result;
    result.message = detail::console_safe(public_message.empty()
        ? "No se pudo completar la operación." : public_message);
    result.reference = detail::console_safe(error.operation);
    if (environment == Environment::development) {
        result.detail = "Código: " + detail::console_safe(error.code) + "\nCausa: " +
            detail::console_safe(error.message) + '\n';
        if (error.context.origin == TraceOrigin::exception_construction) {
            result.detail += "Excepción construida en: " +
                detail::console_safe(error.context.location.file_name()) + ':' +
                std::to_string(error.context.location.line()) + "\nFunción: " +
                detail::console_safe(error.context.location.function_name()) + '\n';
        } else {
            result.detail += "Origen original no disponible; traza del manejador.\n";
        }
        result.detail += "Módulo: " + detail::console_safe(error.module) + '\n';
        for (const auto& field : error.fields)
            result.detail += detail::console_safe(field.key) + ": " + detail::console_safe(field.value) + '\n';
        result.detail += error.context.stack.format();
        for (const auto& cause : error.chain.causes) {
            result.detail += "\nCausa anidada: " + detail::console_safe(cause.message) + '\n';
            if (!cause.code.empty()) result.detail += "Código: " + detail::console_safe(cause.code) + '\n';
            if (cause.context.location.line())
                result.detail += detail::console_safe(cause.context.location.file_name()) + ':' +
                    std::to_string(cause.context.location.line()) + '\n';
        }
        if (error.chain.truncated) result.detail += "\nCadena de causas truncada.\n";
        if (error.report_status == ReportStatus::failed)
            result.detail += "\nNo se pudo escribir el diagnóstico.\n";
    }
    return result;
}

std::string RenderedError::console_text() const {
    return message + "\nReferencia: " + reference + '\n' + detail;
}

} // namespace kotavel
