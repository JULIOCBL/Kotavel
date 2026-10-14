#include <kotavel/foundation/application.hpp>
#include <algorithm>
#include <cstdio>

namespace kotavel {
// Retener y presentar diagnósticos sin mezclar esta política con las transiciones de estado.
namespace {
std::string bounded_message(std::string_view message) {
    auto size = std::min(message.size(), std::size_t{4096});
    if (size < message.size()) {
        while (size > 0 && (static_cast<unsigned char>(message[size]) & 0xc0) == 0x80) --size;
    }
    return std::string(message.substr(0, size));
}
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

} // namespace kotavel
