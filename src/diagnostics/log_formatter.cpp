#include "log_formatter.hpp"
#include "../platform/utc_time.hpp"

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
        case LogLevel::warning: return "warning";
        case LogLevel::error: return "error";
        case LogLevel::critical: return "critical";
        case LogLevel::alert: return "alert";
        case LogLevel::emergency: return "emergency";
    }
    return nullptr;
}

namespace {
std::size_t safe_prefix(std::string_view input, std::size_t limit) {
    auto length = std::min(input.size(), limit);
    if (length < input.size()) {
        while (length > 0 && (static_cast<unsigned char>(input[length]) & 0xc0) == 0x80)
            --length;
    }
    return length;
}

// Mantiene el registro en una línea y evita que el mensaje inyecte entradas falsas.
std::string single_line(std::string_view input, std::size_t limit) {
    const auto length = safe_prefix(input, limit);
    std::string output;
    output.reserve(length + 16);
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned char character : input.substr(0, length)) {
        if (character == '\n') output += "\\n";
        else if (character == '\r') output += "\\r";
        else if (character == '\t') output += "\\t";
        else if (character < 32 || character == 127) {
            output += "\\u00";
            output += hex[character >> 4];
            output += hex[character & 15];
        } else output += static_cast<char>(character);
    }
    if (length < input.size()) output += "...[truncated]";
    return output;
}

std::string multi_line(std::string_view input, std::size_t limit) {
    const auto length = safe_prefix(input, limit);
    std::string output;
    output.reserve(length + 16);
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned char character : input.substr(0, length)) {
        if (character == '\n') output += '\n';
        else if (character == '\r') output += "\\r";
        else if (character == '\t') output += "\\t";
        else if (character < 32 || character == 127) {
            output += "\\u00";
            output += hex[character >> 4];
            output += hex[character & 15];
        } else output += static_cast<char>(character);
    }
    if (length < input.size()) output += "...[truncated]";
    return output;
}

// Codifica valores del objeto de contexto que acompaña el formato Laravel.
std::string json_string(std::string_view input, std::size_t limit) {
    const auto length = safe_prefix(input, limit);
    std::string output = "\"";
    output.reserve(length + 18);
    constexpr char hex[] = "0123456789abcdef";
    for (unsigned char character : input.substr(0, length)) {
        switch (character) {
            case '"': output += "\\\""; break;
            case '\\': output += "\\\\"; break;
            case '\n': output += "\\n"; break;
            case '\r': output += "\\r"; break;
            case '\t': output += "\\t"; break;
            case '\b': output += "\\b"; break;
            case '\f': output += "\\f"; break;
            default:
                if (character < 32 || character == 127) {
                    output += "\\u00";
                    output += hex[character >> 4];
                    output += hex[character & 15];
                } else output += static_cast<char>(character);
        }
    }
    if (length < input.size()) output += "...[truncated]";
    return output + '"';
}

std::string exception_details(const LogRecord& record, std::size_t field_limit,
                              std::size_t trace_limit) {
    std::string result;
    if (record.diagnostic) {
        const auto& diagnostic = *record.diagnostic;
        result = diagnostic.origin == TraceOrigin::exception_construction
            ? "[object] (kotavel::DiagnosticError"
            : "[object] (Exception";
        if (!record.code.empty()) result += "(code: " + single_line(record.code, 512) + ')';
        result += ": " + single_line(record.message, field_limit);
        if (diagnostic.location.line()) {
            result += " at " + single_line(diagnostic.location.file_name(), 256) + ':' +
                std::to_string(diagnostic.location.line());
        }
        const auto function = diagnostic.location.function_name();
        if (function && function[0] != '\0')
            result += " in " + single_line(function, 256);
        result += ")\n[stacktrace]\n" +
            multi_line(diagnostic.stack.format(), trace_limit);
    }

    for (const auto& cause : record.causes) {
        if (!result.empty()) result += '\n';
        result += "[previous exception] (";
        result += cause.code.empty() ? "std::exception"
            : "kotavel::DiagnosticError(code: " + single_line(cause.code, 512) + ')';
        result += ": " + single_line(cause.message, field_limit);
        if (cause.context.location.line()) {
            result += " at " + single_line(cause.context.location.file_name(), 256) + ':' +
                std::to_string(cause.context.location.line());
        }
        result += ')';
        if (cause.context.location.line() || cause.context.stack.size) {
            result += "\n[stacktrace]\n" + multi_line(cause.context.stack.format(), trace_limit);
        }
    }
    if (record.causes_truncated) {
        if (!result.empty()) result += '\n';
        result += "[exception chain truncated]";
    }
    return result;
}

std::string extra_fields(const LogRecord& record, std::size_t field_limit,
                         std::size_t trace_limit, bool payload_truncated) {
    std::string fields;
    bool first = true;
    auto append = [&](std::string_view key, std::string value) {
        if (!first) fields += ',';
        first = false;
        fields += json_string(key, 64) + ':' + std::move(value);
    };
    if (!record.code.empty()) append("code", json_string(record.code, 512));
    if (!record.operation.empty()) append("operation", json_string(record.operation, 512));
    if (!record.fields.empty()) {
        std::string context = "{";
        bool first_context = true;
        for (const auto& field : record.fields) {
            if (!first_context) context += ',';
            first_context = false;
            context += json_string(field.key, 64) + ':' + json_string(field.value, std::min(field_limit, std::size_t{256}));
        }
        context += '}';
        append("context", std::move(context));
    }
    const auto exception = exception_details(record, field_limit, trace_limit);
    if (!exception.empty()) append("exception", json_string(exception, exception.size()));
    if (record.causes_truncated) append("causes_truncated", "true");
    if (payload_truncated) append("payload_truncated", "true");
    return fields.empty() ? std::string{} : "{" + fields + "}";
}
}

// Formato de texto Laravel: [fecha] canal.NIVEL: mensaje {contexto adicional}.
std::string format_log_record(const LogRecord& record, std::string_view module,
                             const char* name, std::uintmax_t max_bytes) {
    const auto now = std::time(nullptr);
    std::tm utc{};
    if (!platform::detail::to_utc_time(now, utc)) throw std::runtime_error("Invalid clock");
    char timestamp[32];
    if (!std::strftime(timestamp, sizeof(timestamp), "%Y-%m-%d %H:%M:%S", &utc))
        throw std::runtime_error("Invalid timestamp");

    std::string level(name);
    for (auto& character : level)
        if (character >= 'a' && character <= 'z') character -= ('a' - 'A');

    const auto prefix = "[" + std::string(timestamp) + "] " + single_line(module, 64) +
        "." + level + ": " + single_line(record.message, 512);
    std::size_t field_limit = 512;
    std::size_t trace_limit = 4096;
    bool payload_truncated = false;
    for (;;) {
        const auto extra = extra_fields(record, field_limit, trace_limit, payload_truncated);
        auto line = prefix;
        if (!extra.empty()) line += ' ' + extra;
        line += '\n';
        if (line.size() <= max_bytes) return line;

        payload_truncated = true;
        if (trace_limit) trace_limit /= 2;
        else if (field_limit) field_limit /= 2;
        else throw std::runtime_error("Diagnostic exceeds file limit");
    }
}
}
