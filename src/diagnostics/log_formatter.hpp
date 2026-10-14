#pragma once
#include <kotavel/diagnostics/logger.hpp>

namespace kotavel::detail {
// Serialización privada: recibe registros ya validados y respeta el límite de archivo.
const char* level_name(LogLevel level) noexcept;
std::string format_log_record(const LogRecord& record, std::string_view module,
                             const char* name, std::uintmax_t max_bytes);
}
