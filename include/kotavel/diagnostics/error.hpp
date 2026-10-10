#pragma once

#include <array>
#include <cstddef>
#include <kotavel/diagnostics/source_location.hpp>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>
#include <exception>
#include <span>

namespace kotavel {

// Afecta solo al hilo actual y restaura su valor al salir (también al lanzar).
class TraceCaptureScope final {
public:
    explicit TraceCaptureScope(bool enabled) noexcept;
    ~TraceCaptureScope();
    TraceCaptureScope(const TraceCaptureScope&) = delete;
    TraceCaptureScope& operator=(const TraceCaptureScope&) = delete;
private:
    bool previous_;
};

// Captura acotada. Los símbolos se resuelven solo al formatear un diagnóstico.
struct StackTrace {
    std::array<void*, 24> frames{};
    std::size_t size = 0;
    bool enabled = true;
    [[nodiscard]] static StackTrace capture() noexcept;
    [[nodiscard]] std::string format() const;
};

enum class TraceOrigin { exception_construction, handler };
struct DiagnosticContext {
    SourceLocation location{};
    TraceOrigin origin = TraceOrigin::handler;
    StackTrace stack;
};

class DiagnosticError : public std::runtime_error {
public:
    DiagnosticError(std::string code, std::string message,
                    SourceLocation location = SourceLocation::current());
    [[nodiscard]] const std::string& code() const noexcept { return code_; }
    [[nodiscard]] const DiagnosticContext& context() const noexcept { return context_; }
private:
    std::string code_;
    DiagnosticContext context_;
};

// Solo declarar recuperable si el estado compartido sigue siendo válido.
class RecoverableError : public DiagnosticError {
public:
    explicit RecoverableError(std::string message,
                              SourceLocation location = SourceLocation::current());
    RecoverableError(std::string code, std::string message,
                     SourceLocation location = SourceLocation::current());
};

enum class ReportStatus { written, filtered, throttled, ignored, failed };
struct ContextField { std::string key; std::string value; };
// Se valida antes de ejecutar: hasta 16 campos únicos de 64/256 bytes.
void validate_context(std::span<const ContextField> fields);
struct OperationContext {
    std::string_view id;
    std::string_view module = "application";
    std::span<const ContextField> fields;
};
struct ErrorCause {
    std::string code;
    std::string message;
    DiagnosticContext context;
};
struct CauseChain {
    std::vector<ErrorCause> causes;
    bool truncated = false;
};
[[nodiscard]] CauseChain collect_causes(std::exception_ptr error);

enum class Environment { production, development };
struct ErrorDetails {
    std::string code;
    std::string operation;
    std::string message;
    DiagnosticContext context;
    bool recoverable = false;
    std::string module = "application";
    std::vector<ContextField> fields;
    CauseChain chain;
    ReportStatus report_status = ReportStatus::ignored;
};

struct RenderedError {
    std::string message;
    std::string reference;
    std::string detail;
    int exit_code = 1;
    [[nodiscard]] std::string console_text() const;
};

// public_message es texto seguro definido por el desarrollador, nunca what().
[[nodiscard]] RenderedError render_error(const ErrorDetails& error,
    Environment environment = Environment::production, std::string_view public_message = {});

} // namespace kotavel
