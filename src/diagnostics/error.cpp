#include <kotavel/diagnostics/error.hpp>
#include <algorithm>
#include <cstdlib>
#include <memory>
#include <sstream>
#include <utility>

#if defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#elif defined(KOTAVEL_HAS_BACKTRACE)
#include <execinfo.h>
#endif

namespace kotavel {
namespace {
thread_local bool capture_enabled = true;
std::string console_safe(std::string_view value) {
    std::string result;
    for (unsigned char c : value) {
        if (c < 32 || c == 127) result += ' ';
        else result += static_cast<char>(c);
    }
    return result;
}
}

TraceCaptureScope::TraceCaptureScope(bool enabled) noexcept : previous_(capture_enabled) {
    capture_enabled = enabled;
}
TraceCaptureScope::~TraceCaptureScope() { capture_enabled = previous_; }

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

StackTrace StackTrace::capture() noexcept {
    StackTrace result;
    result.enabled = capture_enabled;
    if (!capture_enabled) return result;
#if defined(_WIN32)
    result.size = CaptureStackBackTrace(1, static_cast<DWORD>(result.frames.size()),
                                       result.frames.data(), nullptr);
#elif defined(KOTAVEL_HAS_BACKTRACE)
    const auto count = ::backtrace(result.frames.data(), static_cast<int>(result.frames.size()));
    if (count > 0) result.size = static_cast<std::size_t>(count);
#endif
    return result;
}

std::string StackTrace::format() const {
    const auto count = std::min(size, frames.size());
    if (!enabled) return "Traza desactivada";
    if (!count) return "Traza no disponible";
    std::ostringstream out;
#if defined(KOTAVEL_HAS_BACKTRACE)
    // backtrace_symbols entrega memoria de malloc; liberarla incluso al lanzar.
    std::unique_ptr<char*, decltype(&std::free)> symbols(
        ::backtrace_symbols(frames.data(), static_cast<int>(count)), &std::free);
#endif
    for (std::size_t i = 0; i < count; ++i) {
        out << '#' << i << ' ';
#if defined(KOTAVEL_HAS_BACKTRACE)
        if (symbols) out << console_safe(std::string_view(symbols.get()[i]).substr(0, 256));
        else
#endif
            out << frames[i];
        out << '\n';
    }
    return out.str();
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

RenderedError render_error(const ErrorDetails& error, Environment environment,
                            std::string_view public_message) {
    if (environment != Environment::production && environment != Environment::development)
        throw std::invalid_argument("Invalid presentation environment");
    RenderedError result;
    result.message = console_safe(public_message.empty()
        ? "No se pudo completar la operación." : public_message);
    result.reference = console_safe(error.operation);
    if (environment == Environment::development) {
        result.detail = "Código: " + console_safe(error.code) + "\nCausa: " +
            console_safe(error.message) + '\n';
        if (error.context.origin == TraceOrigin::exception_construction) {
            result.detail += "Excepción construida en: " +
                console_safe(error.context.location.file_name()) + ':' +
                std::to_string(error.context.location.line()) + "\nFunción: " +
                console_safe(error.context.location.function_name()) + '\n';
        } else {
            result.detail += "Origen original no disponible; traza del manejador.\n";
        }
        result.detail += "Módulo: " + console_safe(error.module) + '\n';
        for (const auto& field : error.fields)
            result.detail += console_safe(field.key) + ": " + console_safe(field.value) + '\n';
        result.detail += error.context.stack.format();
        for (const auto& cause : error.chain.causes) {
            result.detail += "\nCausa anidada: " + console_safe(cause.message) + '\n';
            if (!cause.code.empty()) result.detail += "Código: " + console_safe(cause.code) + '\n';
            if (cause.context.location.line())
                result.detail += console_safe(cause.context.location.file_name()) + ':' +
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
