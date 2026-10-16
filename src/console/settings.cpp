#include <kotavel/console/runtime.hpp>
#include <array>
#include "settings_error.hpp"

namespace kotavel {
namespace {
template<class T> T read(const Config& config, std::string_view key, T fallback) {
    // Valores presentes de otro tipo se rechazan; el default solo cubre ausencia.
    const auto* value = config.find(key);
    if (!value) return fallback;
    if (const auto* typed = std::get_if<T>(value)) return *typed;
    throw detail::ConsoleSettingsError(std::string(key), "el tipo del valor no coincide con el esperado");
}
LogLevel level(std::string_view name) {
    constexpr std::array names{"trace", "debug", "info", "notice", "warning",
                               "error", "critical", "alert", "emergency"};
    for (std::size_t i = 0; i < names.size(); ++i)
        if (name == names[i]) return static_cast<LogLevel>(i);
    throw detail::ConsoleSettingsError("logging.level",
        "usa trace, debug, info, notice, warning, error, critical, alert o emergency");
}
}
ConsoleSettings console_settings(const ConsoleContext& context) {
    const Config empty;
    const auto& config = context.configuration ? *context.configuration : empty;
    ConsoleSettings result;
    auto& logs = result.logs;
    const auto channel = read(config, "logging.channel", std::string("single"));
    if (channel != "single" && channel != "stderr" && channel != "null")
        throw detail::ConsoleSettingsError("logging.channel", "usa single, stderr o null");
    // Cada canal determina sus destinos; null conserva el manejo de errores.
    logs.enabled = channel != "null";
    logs.file = channel == "single";
    logs.console = channel == "stderr";
    logs.min_level = level(read(config, "logging.level", std::string("info")));
    logs.module = read(config, "logging.module", std::string("app"));
    if (logs.module.empty() || logs.module.size() > 64)
        throw detail::ConsoleSettingsError("logging.module", "debe tener entre 1 y 64 bytes");
    const auto directory = read(config, "logging.directory", std::string("storage/logs"));
    if (directory.empty()) throw detail::ConsoleSettingsError("logging.directory", "la ruta no puede estar vacía");
    // Una ruta relativa pertenece al consumidor; una absoluta se respeta.
    logs.directory = context.root / std::filesystem::path(directory);
    const auto max_bytes = read(config, "logging.max_bytes", 512 * 1024 * 1024);
    const auto backups = read(config, "logging.backups", 3);
    if (max_bytes < 16384)
        throw detail::ConsoleSettingsError("logging.max_bytes", "el mínimo es 16384 bytes");
    if (backups < 0 || backups > 100)
        throw detail::ConsoleSettingsError("logging.backups", "usa un valor entre 0 y 100");
    logs.max_bytes = static_cast<std::size_t>(max_bytes);
    logs.backups = static_cast<unsigned>(backups);

    auto& metrics = result.metrics;
    metrics.enabled = read(config, "metrics.enabled", false);
    metrics.console = read(config, "metrics.console", true);
    metrics.startup = read(config, "metrics.startup", true);
    metrics.stream = read(config, "metrics.stream", false);
    const auto interval_ms = read(config, "metrics.interval_ms", 1000);
    if (interval_ms < 100 || interval_ms > 60000)
        throw detail::ConsoleSettingsError("metrics.interval_ms", "usa un valor entre 100 y 60000 milisegundos");
    metrics.interval = std::chrono::milliseconds(interval_ms);
    return result;
}
}
