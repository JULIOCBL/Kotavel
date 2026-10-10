#pragma once

#include <kotavel/diagnostics/logger.hpp>
#include <functional>
#include <memory>
#include <map>
#include <optional>
#include <stdexcept>
#include <string_view>

namespace kotavel {

// Reglas declaradas por código: afectan reporte y mensaje, nunca recuperación.
struct ErrorRule {
    bool report = true;
    std::optional<LogLevel> level;
    std::string public_message;
};
struct ErrorHandlingOptions {
    Environment environment = Environment::production;
    bool capture_traces = true;
    std::map<std::string, ErrorRule, std::less<>> rules;
};

enum class ApplicationState { created, ready, running, failed, stopped };
enum class OperationResult { success, recovered_error, fatal_error };

// Ciclo de vida síncrono: utilizar desde un único hilo, sin ejecución reentrante.
// El logger serializa sus escrituras; eso no vuelve concurrente a Application.
class Application final {
public:
    explicit Application(LogOptions options, ErrorHandlingOptions errors = {});
    ~Application();
    Application(const Application&) = delete;
    Application& operator=(const Application&) = delete;

    void boot();
    [[nodiscard]] OperationResult execute(std::string_view operation,
                                          const std::function<void()>& action);
    [[nodiscard]] OperationResult execute(const OperationContext& operation,
                                          const std::function<void()>& action);
    void shutdown() noexcept;
    // Se borra al iniciar la siguiente operación. No conservar la referencia.
    [[nodiscard]] const std::optional<ErrorDetails>& last_error() const noexcept { return last_error_; }
    [[nodiscard]] std::optional<RenderedError> present_error() const;
    [[nodiscard]] ApplicationState state() const noexcept { return state_; }
private:
    bool record_error(std::string_view code, const OperationContext& operation,
                      std::string_view message, DiagnosticContext context, bool recoverable) noexcept;
    LogOptions options_;
    ErrorHandlingOptions errors_;
    std::optional<ErrorDetails> last_error_;
    std::unique_ptr<Logger> logger_;
    ApplicationState state_ = ApplicationState::created;
};

} // namespace kotavel
