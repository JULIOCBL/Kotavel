#pragma once

#include <kotavel/diagnostics/error.hpp>
#include <atomic>
#include <chrono>
#include <filesystem>
#include <memory>
#include <mutex>
#include <span>
#include <string_view>
#include <unordered_map>

namespace kotavel {

enum class LogLevel { trace, debug, info, notice, warning, error, critical, alert, emergency };

struct ThrottleOptions {
    std::size_t max_reports = 0; // Cero desactiva el límite.
    std::chrono::milliseconds interval{60000};
    std::size_t max_keys = 256; // Memoria acotada incluso con códigos variables.
};
struct LogOptions {
    std::filesystem::path directory;
    std::size_t max_bytes = std::size_t{512} * 1024 * 1024;
    unsigned backups = 3;
    bool console = true;
    LogLevel min_level = LogLevel::info;
    std::string module = "application";
    ThrottleOptions throttle{};
    // Destinos opcionales: stderr y null no deben crear ni bloquear archivos.
    bool file = true;
    bool enabled = true;
};
struct LogRecord {
    LogLevel level = LogLevel::error;
    std::string_view code;
    std::string_view operation;
    std::string_view message;
    std::string_view module; // Vacío utiliza LogOptions::module.
    std::span<const ContextField> fields;
    const DiagnosticContext* diagnostic = nullptr;
    std::span<const ErrorCause> causes;
    bool causes_truncated = false;
};
struct LogStats {
    std::uint64_t written, filtered, throttled, failed;
};
class DirectoryLease;

class Logger final {
public:
    explicit Logger(LogOptions options);
    ~Logger();
    // Distingue escritura, filtrado, limitación y fallo; no lanza excepciones.
    [[nodiscard]] ReportStatus write(const LogRecord& record) noexcept;
    [[nodiscard]] LogStats stats() const noexcept;
    // Cambia el límite y reinicia sus ventanas, conservando los contadores.
    void configure_report_throttling(ThrottleOptions options);
    // Compatibilidad: true incluye mensajes filtrados o limitados.
    bool report(LogLevel level, std::string_view code, std::string_view operation,
                std::string_view message, const DiagnosticContext* context = nullptr) noexcept;
    bool report(std::string_view code, std::string_view operation,
                std::string_view message) noexcept;
private:
    struct Window { std::chrono::steady_clock::time_point start; std::size_t reports; };
    bool limited(const LogRecord& record, std::string_view module);
    LogOptions options_;
    std::unique_ptr<DirectoryLease> lease_;
    std::mutex mutex_;
    std::unordered_map<std::string, Window> windows_;
    std::atomic<std::uint64_t> written_{0}, filtered_{0}, throttled_{0}, failed_{0};
};

} // namespace kotavel
