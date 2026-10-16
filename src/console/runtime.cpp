#include <kotavel/console/runtime.hpp>
#include "arguments.hpp"
#include <iomanip>
#include <iostream>
#include <mutex>
#include <sstream>
#include <utility>

namespace kotavel {
namespace {
std::mutex metrics_output_mutex;
}
int run_console(int argc, char** argv, const ExecuteConsole& execute,
                std::string_view application_help, const LoadConsoleConfig& load_config) {
    return run_console(argc, argv, console_settings, execute, application_help, load_config);
}
int run_console(int argc, char** argv, const ConfigureConsole& configure,
                const ExecuteConsole& execute, std::string_view application_help,
                const LoadConsoleConfig& load_config) {
    using Clock = std::chrono::steady_clock;
    const auto run_started = Clock::now();
    bool debug = false;
    try {
        if (argc < 1 || !argv || !configure || !execute)
            throw std::invalid_argument("Console requires arguments and callbacks");
        auto options = detail::parse_console_arguments(argc, argv);
        debug = options.debug;
        if (options.help) {
            std::cout << "Uso: aplicación [--env=local|development|production] [--project-root=RUTA] [--development]\n";
            if (!application_help.empty()) std::cout << application_help << '\n';
            return 0;
        }
        auto root = std::move(options.root);
        if (root.empty()) root = std::filesystem::current_path();
        root = std::filesystem::canonical(root);
        if (!std::filesystem::is_directory(root)) throw std::invalid_argument("Project root must be a directory");
        metrics::StartupMetrics startup;
        // Etapa 1: cargar y validar el entorno antes de cualquier configuración.
        const auto environment_started = Clock::now();
        FileEnvironment source(root, options.environment);
        const Env reader(source);
        // Los helpers del consumidor consultan el mismo archivo y precedencia
        // durante este arranque; RAII restaura la fuente incluso si hay un fallo.
        const EnvScope environment_scope(reader);
        auto identity = AppEnvironment::load(reader);
        startup.environment = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - environment_started);
        debug = debug || identity.debug;
        // Etapa 2: cargar mapas de configuración usando el entorno ya disponible.
        // Su ámbito incluye configure, execute y cierre; nunca se propaga a otros hilos.
        const auto configuration_started = Clock::now();
        Config settings_map = load_config ? load_config() : Config{};
        startup.configuration = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - configuration_started);
        const ConfigScope config_scope(settings_map);
        ConsoleContext context{std::move(root), std::move(identity), options.arguments, &settings_map};
        const auto settings_started = Clock::now();
        auto settings = configure(context);
        // La presentación detallada depende del entorno resuelto, no de cada comando.
        settings.errors.environment = debug ? Environment::development : Environment::production;
        startup.settings = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - settings_started);
        const auto boot_started = Clock::now();
        Application application(std::move(settings.logs), std::move(settings.errors));
        application.boot();
        startup.application_boot = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - boot_started);
        startup.total = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - run_started);

        // El stream local permite observar métricas desde otra terminal.
        std::optional<metrics::PerformanceStream> stream;
        bool print_metrics_to_console = settings.performance.console;
        if (settings.performance.enabled && settings.performance.stream) {
            try {
                stream.emplace();
                stream->start(context.root);
            } catch (const std::exception& error) {
                // El canal de observación es opcional; un fallo no debe detener la aplicación.
                std::cerr << "[Kotavel] No se pudo abrir el monitor local; se mostrarán métricas aquí.\n";
                if (debug) std::cerr << error.what() << '\n';
                print_metrics_to_console = true;
                stream.reset();
            }
        }

        // El monitor arranca cuando Application está lista y se detiene antes del cierre.
        metrics::PerformanceMonitor monitor;
        if (settings.performance.enabled && settings.performance.startup) {
            if (stream && stream->running()) stream->publish_startup(startup);
            if (print_metrics_to_console) {
                const auto milliseconds = [](std::chrono::nanoseconds value) {
                    return std::chrono::duration<double, std::milli>(value).count();
                };
                std::lock_guard output_lock(metrics_output_mutex);
                std::ostringstream output;
                output << std::fixed << std::setprecision(2)
                       << "[Kotavel] Arranque: entorno " << milliseconds(startup.environment)
                       << " ms | configuración " << milliseconds(startup.configuration)
                       << " ms | opciones " << milliseconds(startup.settings)
                       << " ms | aplicación " << milliseconds(startup.application_boot)
                       << " ms | total " << milliseconds(startup.total) << " ms\n";
                std::cout << output.str() << std::flush;
            }
        }
        if (settings.performance.enabled && (print_metrics_to_console || (stream && stream->running()))) {
            const auto publish_sample = [&](const metrics::PerformanceSample& sample) {
                if (stream && stream->running()) stream->publish(sample);
                if (!print_metrics_to_console) return;
                std::lock_guard output_lock(metrics_output_mutex);
                std::ostringstream output;
                output << "[Kotavel] CPU: ";
                if (sample.process_cpu_percent)
                    output << std::fixed << std::setprecision(1) << *sample.process_cpu_percent << " %";
                else
                    output << "calculando";
                output << " | RAM residente: " << std::fixed << std::setprecision(1)
                       << static_cast<double>(sample.resident_memory_bytes) / (1024.0 * 1024.0)
                       << " MiB\n";
                std::cout << output.str() << std::flush;
            };
            // Mostrar la RAM inicial aunque el monitor se conecte después.
            metrics::ProcessMetricsCollector initial_collector;
            if (const auto initial = initial_collector.sample()) publish_sample(*initial);
            monitor.start(settings.performance.interval, publish_sample, std::move(initial_collector));
        }
        const auto code = execute(application, context);
        // No convertir un fallo fatal ignorado por el consumidor en una salida exitosa.
        const bool failed = application.state() == ApplicationState::failed ||
            (application.last_error() && !application.last_error()->recoverable);
        monitor.stop();
        if (stream) stream->stop();
        application.shutdown();
        return failed && code == 0 ? 1 : code;
    } catch (const std::exception& error) {
        // Application libera sus recursos por RAII antes de informar un fallo externo.
        std::cerr << "No se pudo iniciar o ejecutar Kotavel.\n";
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (...) {
        std::cerr << "Kotavel terminó por un error inesperado.\n";
        return 1;
    }
}
}
