#include <kotavel/foundation/application.hpp>
#include <utility>

namespace kotavel {
// Coordina estados, ejecución y liberación del logger. El reporte vive en application_errors.cpp.
Application::Application(LogOptions options, ErrorHandlingOptions errors)
    : options_(std::move(options)), errors_(std::move(errors)) {
    if (errors_.environment != Environment::production && errors_.environment != Environment::development)
        throw std::invalid_argument("Invalid presentation environment");
    for (const auto& [code, rule] : errors_.rules) {
        if (code.empty() || code.size() > 512 || (rule.level && (*rule.level < LogLevel::trace || *rule.level > LogLevel::emergency)))
            throw std::invalid_argument("Invalid error rule");
    }
}
Application::~Application() { shutdown(); }

void Application::boot() {
    if (state_ != ApplicationState::created)
        throw std::logic_error("Application can only boot once");
    try {
        logger_ = std::make_unique<Logger>(options_);
        state_ = ApplicationState::ready;
    } catch (...) {
        state_ = ApplicationState::failed;
        throw; // El consumidor informa el fallo de arranque y decide su salida.
    }
}

OperationResult Application::execute(std::string_view operation,
                                      const std::function<void()>& action) {
    return execute(OperationContext{operation, options_.module, {}}, action);
}

OperationResult Application::execute(const OperationContext& operation,
                                      const std::function<void()>& action) {
    if (state_ != ApplicationState::ready)
        throw std::logic_error("Application is not ready");
    if (operation.id.empty() || operation.id.size() > 512 || !action)
        throw std::invalid_argument("Operation requires an identifier of 1 to 512 bytes and an action");
    if (operation.module.empty() || operation.module.size() > 64)
        throw std::invalid_argument("Operation module requires 1 to 64 bytes");
    validate_context(operation.fields);
    TraceCaptureScope trace_scope(errors_.capture_traces);
    last_error_.reset();
    state_ = ApplicationState::running;
    try {
        action();
        state_ = ApplicationState::ready;
        return OperationResult::success;
    } catch (const RecoverableError& error) {
        // Mantener failed hasta completar el diagnóstico interno; incluso una
        // asignación fallida durante el manejo del error debe dejar estado definido.
        state_ = ApplicationState::failed;
        if (record_error(error.code(), operation, error.what(), error.context(), true)) {
            state_ = ApplicationState::ready;
            return OperationResult::recovered_error;
        }
    } catch (const DiagnosticError& error) {
        state_ = ApplicationState::failed;
        record_error(error.code(), operation, error.what(), error.context(), false);
    } catch (const std::exception& error) {
        state_ = ApplicationState::failed;
        record_error("operation.unexpected", operation, error.what(),
                     {{}, TraceOrigin::handler, StackTrace::capture()}, false);
    } catch (...) {
        state_ = ApplicationState::failed;
        record_error("operation.unknown", operation, "Unknown exception",
                     {{}, TraceOrigin::handler, StackTrace::capture()}, false);
    }
    return OperationResult::fatal_error;
}

void Application::shutdown() noexcept {
    // No destruir el logger mientras una operación todavía lo puede necesitar.
    // La parada se solicita desde el llamador, después de execute().
    if (state_ == ApplicationState::running) return;
    logger_.reset();
    state_ = ApplicationState::stopped;
}

} // namespace kotavel
