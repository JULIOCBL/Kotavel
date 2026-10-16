#pragma once
#include <kotavel/foundation/application.hpp>
#include <kotavel/env/env.hpp>
#include <kotavel/config/config_helpers.hpp>
#include <kotavel/metrics/performance.hpp>
#include <span>
#include <vector>

namespace kotavel {
struct ConsoleContext {
    std::filesystem::path root;
    AppEnvironment identity;
    std::span<const std::string_view> arguments;
    Config* configuration = nullptr;
};
struct ConsoleSettings {
    LogOptions logs;
    ErrorHandlingOptions errors;
    metrics::PerformanceOptions performance;
};
using ConfigureConsole = std::function<ConsoleSettings(const ConsoleContext&)>;
using ExecuteConsole = std::function<int(Application&, const ConsoleContext&)>;
using LoadConsoleConfig = std::function<Config()>;
// Construcción estándar desde logging.*; el consumidor solo declara valores.
ConsoleSettings console_settings(const ConsoleContext& context);
// Callbacks síncronos; no conservar context ni sus vistas después del retorno.
// La ayuda no requiere cargar archivos, crear el logger ni iniciar la aplicación.
int run_console(int argc, char** argv, const ConfigureConsole& configure,
                const ExecuteConsole& execute, std::string_view application_help = {},
                const LoadConsoleConfig& load_config = {});
// Arranque habitual con la configuración del núcleo y el registro del consumidor.
int run_console(int argc, char** argv, const ExecuteConsole& execute,
                std::string_view application_help = {},
                const LoadConsoleConfig& load_config = {});
OperationResult execute_console(Application& application, std::string_view id,
                                const std::function<void()>& operation);
}
