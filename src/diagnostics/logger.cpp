#include <kotavel/diagnostics/logger.hpp>
#include "directory_lease.hpp"
#include <algorithm>
#include <cstdio>
#include <ctime>
#include <fstream>
#include <stdexcept>
#include <string>
#include <utility>

namespace kotavel {
namespace {
const char* level_name(LogLevel level) noexcept {
    switch (level) {
        case LogLevel::trace: return "trace";
        case LogLevel::debug: return "debug";
        case LogLevel::info: return "info";
        case LogLevel::notice: return "notice";
        case LogLevel::alert: return "alert";
        case LogLevel::emergency: return "emergency";
        case LogLevel::warning: return "warning";
        case LogLevel::error: return "error";
        case LogLevel::critical: return "critical";
    }
    return nullptr;
}

// Limita cada campo y escapa controles para impedir inyección de líneas de log.
std::string quote(std::string_view input, std::size_t limit = 512) {
    std::string out = "\"";
    constexpr char hex[] = "0123456789abcdef";
    auto length = std::min(input.size(), limit);
    // No cortar un carácter UTF-8 entre sus bytes de continuación.
    if (length < input.size()) {
        while (length > 0 && (static_cast<unsigned char>(input[length]) & 0xc0) == 0x80)
            --length;
    }
    for (unsigned char c : input.substr(0, length)) {
        if (c == '"' || c == '\\') { out += '\\'; out += c; }
        else if (c < 32) {
            out += "\\u00"; out += hex[c >> 4]; out += hex[c & 15];
        } else out += c;
    }
    if (input.size() > length) out += "...[truncated]";
    return out + '"';
}

void validate_throttle(const ThrottleOptions& options) {
    if (options.interval.count() <= 0 || options.max_keys == 0 || options.max_keys > 4096)
        throw std::invalid_argument("Throttle requires a positive interval and 1 to 4096 keys");
}
}

Logger::Logger(LogOptions options) : options_(std::move(options)) {
    if (!level_name(options_.min_level))
        throw std::invalid_argument("Invalid minimum log level");
    if (options_.directory.empty() || options_.max_bytes < 16384 || options_.backups > 100)
        throw std::invalid_argument("Logger requires a directory, max_bytes >= 16384 and backups <= 100");
    if (options_.module.empty() || options_.module.size() > 64)
        throw std::invalid_argument("Log module requires 1 to 64 bytes");
    validate_throttle(options_.throttle);
    std::filesystem::create_directories(options_.directory);
    lease_ = std::make_unique<DirectoryLease>(options_.directory);
    std::ofstream probe(options_.directory / "kotavel.log", std::ios::app | std::ios::binary);
    if (!probe) throw std::runtime_error("Cannot open Kotavel log file");
}

Logger::~Logger() = default;

LogStats Logger::stats() const noexcept {
    return {written_.load(), filtered_.load(), throttled_.load(), failed_.load()};
}
void Logger::configure_report_throttling(ThrottleOptions options) {
    validate_throttle(options);
    std::lock_guard lock(mutex_);
    options_.throttle = options;
    windows_.clear();
}

bool Logger::limited(const LogRecord& record, std::string_view module) {
    const auto& config = options_.throttle;
    if (!config.max_reports) return false;
    const auto now = std::chrono::steady_clock::now();
    // Longitudes explícitas evitan colisiones si un campo contiene separadores.
    const auto key = std::to_string(module.size()) + ":" + std::string(module) +
        std::to_string(record.code.size()) + ":" + std::string(record.code) + ":" +
        std::to_string(static_cast<int>(record.level));
    auto found = windows_.find(key);
    if (found == windows_.end()) {
        if (windows_.size() >= config.max_keys) {
            for (auto it = windows_.begin(); it != windows_.end();) {
                if (now - it->second.start >= config.interval) it = windows_.erase(it);
                else ++it;
            }
            if (windows_.size() >= config.max_keys) return true;
        }
        windows_.emplace(key, Window{now, 1});
        return false;
    }
    auto& window = found->second;
    if (now - window.start >= config.interval) window = {now, 0};
    if (window.reports >= config.max_reports) return true;
    ++window.reports;
    return false;
}

bool Logger::report(std::string_view code, std::string_view operation,
                    std::string_view message) noexcept {
    return report(LogLevel::error, code, operation, message);
}
bool Logger::report(LogLevel level, std::string_view code, std::string_view operation,
                    std::string_view message, const DiagnosticContext* context) noexcept {
    return write({level, code, operation, message, {}, {}, context, {}, false}) != ReportStatus::failed;
}

ReportStatus Logger::write(const LogRecord& record) noexcept {
    const auto name = level_name(record.level);
    if (!name) {
        ++failed_;
        std::fputs("Kotavel: invalid diagnostic level.\n", stderr);
        return ReportStatus::failed;
    }
    if (record.level < options_.min_level) { ++filtered_; return ReportStatus::filtered; }
    try {
        std::lock_guard lock(mutex_);
        const auto module = record.module.empty() ? std::string_view(options_.module) : record.module;
        if (module.size() > 64 || record.code.size() > 512 || record.operation.size() > 512 ||
            record.causes.size() > 8) throw std::invalid_argument("Invalid log record size");
        validate_context(record.fields);
        if (limited(record, module)) { ++throttled_; return ReportStatus::throttled; }
        const auto context = record.diagnostic;
        const auto now = std::time(nullptr);
        std::tm utc{};
#ifdef _WIN32
        if (gmtime_s(&utc, &now) != 0) throw std::runtime_error("Invalid clock");
#else
        if (!gmtime_r(&now, &utc)) throw std::runtime_error("Invalid clock");
#endif
        char timestamp[32];
        if (!std::strftime(timestamp, sizeof(timestamp), "%Y-%m-%dT%H:%M:%SZ", &utc))
            throw std::runtime_error("Invalid timestamp");
        auto line = std::string("{\"time\":\"") + timestamp +
            "\",\"level\":\"" + name + "\",\"module\":" + quote(module) + ",\"code\":" + quote(record.code) +
            ",\"operation\":" + quote(record.operation) + ",\"message\":" + quote(record.message);
        std::string extra;
        bool payload_truncated = false;
        for (std::size_t limit = 256;; limit /= 2) {
            extra = ",\"context\":{";
            bool first = true;
            for (const auto& field : record.fields) {
                if (!first) extra += ',';
                first = false;
                extra += quote(field.key, 64) + ':' + quote(field.value, limit);
            }
            extra += "},\"causes\":[";
            first = true;
            for (const auto& cause : record.causes) {
                if (!first) extra += ',';
                first = false;
                extra += "{\"code\":" + quote(cause.code, limit) + ",\"message\":" +
                    quote(cause.message, limit) + ",\"file\":" +
                    quote(cause.context.location.file_name(), limit) + ",\"line\":" +
                    std::to_string(cause.context.location.line()) + '}';
            }
            extra += "]";
            if (line.size() + extra.size() + 4096 <= options_.max_bytes) break;
            payload_truncated = true;
            if (limit == 0) { extra = ",\"context\":{},\"causes\":[]"; break; }
        }
        line += extra + ",\"payload_truncated\":" + (payload_truncated ? "true" : "false");
        line += std::string(",\"causes_truncated\":") + (record.causes_truncated ? "true" : "false");
        if (context) {
            line += ",\"trace_origin\":" + quote(context->origin == TraceOrigin::exception_construction
                ? "exception_construction" : "handler");
            line += ",\"file\":" + quote(context->location.file_name(), 256) +
                ",\"line\":" + std::to_string(context->location.line()) +
                ",\"function\":" + quote(context->location.function_name(), 256);
            // Reducir la traza hasta que el registro completo respete el límite.
            const auto trace = context->stack.format();
            auto limit = std::size_t{4096};
            auto encoded = quote(trace, limit);
            while (line.size() + encoded.size() + 32 > options_.max_bytes && limit > 0) {
                limit /= 2;
                encoded = quote(trace, limit);
            }
            line += ",\"stack\":" + encoded;
        }
        line += "}\n";
        if (line.size() > options_.max_bytes) throw std::runtime_error("Diagnostic exceeds file limit");
        const auto path = options_.directory / "kotavel.log";
        if (std::filesystem::exists(path) &&
            std::filesystem::file_size(path) > options_.max_bytes - line.size()) {
            // Archivos cerrados antes de renombrar: compatible también con Windows.
            for (unsigned i = options_.backups; i > 0; --i) {
                auto dest = path; dest += "." + std::to_string(i);
                auto source = path;
                if (i > 1) source += "." + std::to_string(i - 1);
                std::filesystem::remove(dest);
                if (std::filesystem::exists(source)) std::filesystem::rename(source, dest);
            }
            if (options_.backups == 0) std::filesystem::remove(path);
        }
        std::ofstream file(path, std::ios::app | std::ios::binary);
        file << line;
        file.flush();
        if (!file) throw std::runtime_error("Cannot write Kotavel log file");
        if (options_.console && (std::fwrite(line.data(), 1, line.size(), stderr) != line.size() ||
                                 std::fflush(stderr) != 0))
            throw std::runtime_error("Cannot write diagnostic console");
        ++written_;
        return ReportStatus::written;
    } catch (...) {
        // Sin asignaciones, sin recursión y sin intentar reutilizar el logger.
        std::fputs("Kotavel: diagnostic write failed; message could not be recorded.\n", stderr);
        ++failed_;
        return ReportStatus::failed;
    }
}

} // namespace kotavel
