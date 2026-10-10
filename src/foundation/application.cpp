#include <kotavel/foundation/application.hpp>
#include <utility>
#include <algorithm>
#include <cstdio>

namespace kotavel {
namespace {
std::string bounded_message(std::string_view message) {
    auto size = std::min(message.size(), std::size_t{4096});
    if (size < message.size()) {
        while (size > 0 && (static_cast<unsigned char>(message[size]) & 0xc0) == 0x80) --size;
    }
    return std::string(message.substr(0, size));
}
}

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

bool Application::record_error(std::string_view code, const OperationContext& operation,
                               std::string_view message, DiagnosticContext context,
                               bool recoverable) noexcept {
    try {
        // El estado conservado también está acotado; no retener cargas completas.
        last_error_.emplace(ErrorDetails{std::string(code.substr(0, 512)),
            std::string(operation.id), bounded_message(message),
            context, recoverable, std::string(operation.module),
            {operation.fields.begin(), operation.fields.end()}, collect_causes(std::current_exception()),
            ReportStatus::ignored});
        const auto rule = errors_.rules.find(code);
        if (rule == errors_.rules.end() || rule->second.report) {
            auto level = recoverable ? LogLevel::error : LogLevel::critical;
            if (rule != errors_.rules.end() && rule->second.level) level = *rule->second.level;
            last_error_->report_status = logger_->write({level, code, operation.id, message,
                operation.module, operation.fields, &context, last_error_->chain.causes,
                last_error_->chain.truncated});
        }
        return true;
    } catch (...) {
        last_error_.reset();
        std::fputs("Kotavel: unable to retain error diagnostic; application stopped.\n", stderr);
        return false;
    }
}

std::optional<RenderedError> Application::present_error() const {
    if (!last_error_) return std::nullopt;
    const auto rule = errors_.rules.find(last_error_->code);
    return render_error(*last_error_, errors_.environment,
        rule == errors_.rules.end() ? std::string_view{} : rule->second.public_message);
}

void Application::shutdown() noexcept {
    // No destruir el logger mientras una operación todavía lo puede necesitar.
    // La parada se solicita desde el llamador, después de execute().
    if (state_ == ApplicationState::running) return;
    logger_.reset();
    state_ = ApplicationState::stopped;
}

} // namespace kotavel
