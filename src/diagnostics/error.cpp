#include <kotavel/diagnostics/error.hpp>
#include <algorithm>
#include <utility>

namespace kotavel {
// Contratos de error, validación del contexto y recolección acotada de causas.
void validate_context(std::span<const ContextField> fields) {
    if (fields.size() > 16) throw std::invalid_argument("Context supports up to 16 fields");
    for (std::size_t i = 0; i < fields.size(); ++i) {
        if (fields[i].key.empty() || fields[i].key.size() > 64 || fields[i].value.size() > 256)
            throw std::invalid_argument("Invalid context field size");
        for (std::size_t j = 0; j < i; ++j)
            if (fields[i].key == fields[j].key) throw std::invalid_argument("Duplicate context key");
    }
}

CauseChain collect_causes(std::exception_ptr error) {
    CauseChain result;
    bool root = true;
    while (error) {
        if (!root && result.causes.size() == 8) { result.truncated = true; break; }
        ErrorCause cause;
        std::exception_ptr next;
        try { std::rethrow_exception(error); }
        catch (const std::exception& current) {
            std::string_view message(current.what());
            auto size = std::min(message.size(), std::size_t{512});
            if (size < message.size())
                while (size > 0 && (static_cast<unsigned char>(message[size]) & 0xc0) == 0x80) --size;
            cause.message = message.substr(0, size);
            if (const auto* diagnostic = dynamic_cast<const DiagnosticError*>(&current)) {
                cause.code = diagnostic->code();
                cause.context = diagnostic->context();
            }
            if (const auto* nested = dynamic_cast<const std::nested_exception*>(&current))
                next = nested->nested_ptr(); // Puede estar vacío: no usar rethrow_nested sin comprobar.
        } catch (const std::nested_exception& nested) {
            cause.message = "Unknown nested exception";
            next = nested.nested_ptr();
        } catch (...) { cause.message = "Unknown nested exception"; }
        if (!root) result.causes.push_back(std::move(cause));
        root = false;
        error = next;
    }
    return result;
}

DiagnosticError::DiagnosticError(std::string code, std::string message, SourceLocation location)
    : std::runtime_error(std::move(message)), code_(std::move(code)),
      context_{location, TraceOrigin::exception_construction, StackTrace::capture()} {
    if (code_.empty() || code_.size() > 512)
        throw std::invalid_argument("Diagnostic error requires a code of 1 to 512 bytes");
}
RecoverableError::RecoverableError(std::string message, SourceLocation location)
    : DiagnosticError("operation.recoverable", std::move(message), location) {}
RecoverableError::RecoverableError(std::string code, std::string message, SourceLocation location)
    : DiagnosticError(std::move(code), std::move(message), location) {}

} // namespace kotavel
