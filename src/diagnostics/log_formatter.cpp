#include "log_formatter.hpp"
#include <algorithm>
#include <ctime>
#include <stdexcept>

namespace kotavel::detail {
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
static std::string quote(std::string_view input, std::size_t limit = 512) {
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

// Construir el JSON sin acceder a archivos ni controlar bloqueos del logger.
std::string format_log_record(const LogRecord& record, std::string_view module,
                             const char* name, std::uintmax_t max_bytes) {
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
        if (line.size() + extra.size() + 4096 <= max_bytes) break;
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
        while (line.size() + encoded.size() + 32 > max_bytes && limit > 0) {
            limit /= 2;
            encoded = quote(trace, limit);
        }
        line += ",\"stack\":" + encoded;
    }
    line += "}\n";
    if (line.size() > max_bytes) throw std::runtime_error("Diagnostic exceeds file limit");
    return line;
}
}
