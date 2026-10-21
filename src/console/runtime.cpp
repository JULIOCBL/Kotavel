#include <kotavel/console/runtime.hpp>

#include "arguments.hpp"
#include "settings_error.hpp"
#include "../env/environment_internal.hpp"
#include "../text/messages.hpp"

#include <kotavel/system/os.hpp>
#include <kotavel/diagnostics/bootstrap_logger.hpp>
#include "../platform/platform_services.hpp"
#include "../platform/application_bundle.hpp"

#include <array>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <mutex>
#include <optional>
#include <sstream>
#include <utility>

namespace kotavel {
namespace {
std::mutex metrics_output_mutex;

// Al cerrar, cada proveedor libera primero los recursos que dependen de otros.
class ProviderLifecycle final {
public:
    ProviderLifecycle(Container& services,
                      std::vector<std::unique_ptr<ServiceProvider>> providers)
        : services_(services), providers_(std::move(providers)) {}
    ~ProviderLifecycle() { shutdown(); }
    ProviderLifecycle(const ProviderLifecycle&) = delete;
    ProviderLifecycle& operator=(const ProviderLifecycle&) = delete;

    void register_services() {
        for (const auto& provider : providers_) {
            if (!provider) throw std::invalid_argument("Service provider cannot be null");
            provider->register_services(services_);
        }
    }

    void boot() {
        for (const auto& provider : providers_) {
            // Cerrar también el proveedor cuyo boot falle durante su inicialización.
            ++boot_attempted_;
            provider->boot(services_);
        }
    }

    void shutdown() noexcept {
        while (boot_attempted_ > 0) {
            --boot_attempted_;
            providers_[boot_attempted_]->shutdown(services_);
        }
    }

private:
    Container& services_;
    std::vector<std::unique_ptr<ServiceProvider>> providers_;
    std::size_t boot_attempted_ = 0;
};
}
int run_console(int argc, char** argv, const ExecuteConsole& execute,
                std::string_view application_help, const LoadConsoleConfig& load_config,
                const CreateServiceProviders& create_providers) {
    return run_console(argc, argv, console_settings, execute, application_help,
                       load_config, create_providers);
}
int run_console(int argc, char** argv, const ConfigureConsole& configure,
                const ExecuteConsole& execute, std::string_view application_help,
                const LoadConsoleConfig& load_config,
                const CreateServiceProviders& create_providers) {
    using Clock = std::chrono::steady_clock;
    const auto run_started = Clock::now();
    bool debug = false;
    std::filesystem::path project_root;
    std::filesystem::path log_directory;
    std::string_view phase = "parse_arguments";
    try { log_directory = std::filesystem::current_path() / "storage" / "logs"; }
    catch (...) {}
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
        std::optional<platform::detail::InstalledBundle> bundle;
        if (options.root.empty()) bundle = platform::detail::installed_bundle(argv[0]);
        if (!options.environment_explicit && bundle)
            options.environment = bundle->environment;
        auto root = std::move(options.root);
        if (root.empty())
            root = bundle ? bundle->data : std::filesystem::current_path();
        project_root = root;
        root = std::filesystem::canonical(root);
        if (!std::filesystem::is_directory(root)) throw std::invalid_argument("Project root must be a directory");
        project_root = root;
        log_directory = root / "storage" / "logs";
        metrics::StartupMetrics startup;
        // Etapa 1: cargar y validar el entorno antes de cualquier configuración.
        phase = "load_environment";
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
        phase = "load_configuration";
        const auto configuration_started = Clock::now();
        auto settings_map = std::make_shared<Config>(load_config ? load_config() : Config{});
        startup.configuration = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - configuration_started);
        Container services;
        services.instance<Config>(settings_map);
        services.instance<AppEnvironment>(std::make_shared<AppEnvironment>(identity));
        const ConfigScope config_scope(*settings_map);
        ConsoleContext context{std::move(root), std::move(identity), options.arguments,
                               settings_map.get(), nullptr};
        phase = "configure_console";
        const auto settings_started = Clock::now();
        auto settings = configure(context);
        if (!settings.logs.directory.empty()) log_directory = settings.logs.directory;
        // La presentación detallada depende del entorno resuelto, no de cada comando.
        settings.errors.environment = debug ? Environment::development : Environment::production;
        startup.settings = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - settings_started);
        const auto boot_started = Clock::now();
        phase = "configure_application";
        services.instance<Application>(std::make_shared<Application>(
            std::move(settings.logs), std::move(settings.errors)));
        phase = "register_services";
        ServiceProviderList framework_provider_list;
        framework_provider_list.push_back(platform::detail::make_platform_service_provider());
        ProviderLifecycle framework_providers(services, std::move(framework_provider_list));
        ProviderLifecycle application_providers(services, create_providers ? create_providers()
            : std::vector<std::unique_ptr<ServiceProvider>>{});
        // Primero se registran providers de la aplicación para que puedan sustituir fallbacks.
        application_providers.register_services();
        framework_providers.register_services();
        services.seal();
        Application& application = services.get<Application>();
        // Exponer el contenedor solo después de completar todos los registros.
        context.container = &services;
        phase = "boot_application";
        application.boot();
        // Los proveedores arrancan después de Application y antes del callback consumidor.
        phase = "boot_services";
        framework_providers.boot();
        application_providers.boot();
        startup.application_boot = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - boot_started);
        startup.total = std::chrono::duration_cast<std::chrono::nanoseconds>(
            Clock::now() - run_started);

        // El stream local permite observar métricas desde otra terminal.
        phase = "start_metrics";
        std::optional<metrics::MetricsStream> stream;
        bool print_metrics_to_console = settings.metrics.console;
        if (settings.metrics.enabled && settings.metrics.stream) {
            try {
                stream.emplace();
                stream->start(context.root);
            } catch (const std::exception& error) {
                // El canal de observación es opcional; un fallo no debe detener la aplicación.
                const auto description = Messages::get("runtime.metrics_stream_failed.description");
                const auto action = Messages::get("runtime.metrics_stream_failed.action");
                const std::array<ContextField, 1> fields{{{"reason", error.what()}}};
                const auto status = application.log(LogLevel::warning, "metrics.stream_start_failed",
                                      "start_metrics_stream",
                                      description, fields);
                std::cerr << "[Kotavel] " << description << ' ' << action << '\n';
                if (status == ReportStatus::failed)
                    std::cerr << Messages::get("runtime.diagnostic_not_saved.description") << '\n';
                if (debug) std::cerr << error.what() << '\n';
                print_metrics_to_console = true;
                stream.reset();
            }
        }

        // El monitor arranca cuando Application está lista y se detiene antes del cierre.
        metrics::MetricsMonitor monitor;
        const contracts::ProcessMetricsProvider* process_metrics = nullptr;
        if (settings.metrics.enabled)
            process_metrics = &services.get<contracts::ProcessMetricsProvider>();
        if (settings.metrics.enabled && settings.metrics.startup) {
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
        if (settings.metrics.enabled && (print_metrics_to_console || (stream && stream->running()))) {
            const auto publish_sample = [&](const metrics::MetricsSample& sample) {
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
            metrics::ProcessMetricsCollector initial_collector(*process_metrics);
            if (const auto initial = initial_collector.sample()) publish_sample(*initial);
            monitor.start(settings.metrics.interval, publish_sample, std::move(initial_collector));
        }
        phase = "execute_application";
        const auto code = execute(application, context);
        // No convertir un fallo fatal ignorado por el consumidor en una salida exitosa.
        const bool failed = application.state() == ApplicationState::failed ||
            (application.last_error() && !application.last_error()->recoverable);
        monitor.stop();
        if (stream) stream->stop();
        application_providers.shutdown();
        framework_providers.shutdown();
        application.shutdown();
        services.shutdown();
        return failed && code == 0 ? 1 : code;
    } catch (const detail::EnvironmentFileOpenError& error) {
        const auto file = error.path().filename().string();
        const auto example = file + ".example";
        const std::string message_key = "env.open_failed." + std::string(OS::key()) + ".";
        const auto description = Messages::get(message_key + "description", {{"file", file}});
        // Aún no existe Application; BootstrapLogger encapsula el registro previo al arranque.
        const auto log_status = BootstrapLogger::environment_file_open_failed(
            error.path().parent_path() / "storage" / "logs", file, example,
            error.path().parent_path().string(), error.what(), description);
        std::cerr << description << '\n'
                  << Messages::get(message_key + "action", {{"file", file}, {"example", example}}) << '\n'
                  << Messages::get(message_key + "command", {{"example", example}, {"file", file}}) << '\n';
        if (log_status == ReportStatus::failed)
            std::cerr << Messages::get("runtime.startup_failed.log_failed", {
                {"log_file", (error.path().parent_path() / "storage" / "logs" / "kotavel.log").string()}}) << '\n';
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (const detail::EnvironmentFileInvalidError& error) {
        const auto file = error.path().filename().string();
        const auto description = error.line()
            ? Messages::get("env.invalid.line", {{"file", file}, {"line", std::to_string(error.line())}})
            : Messages::get("env.invalid.file", {{"file", file}});
        const auto log_status = BootstrapLogger::environment_file_invalid(
            log_directory, file, error.path().parent_path().string(), error.line(),
            error.what(), description);
        std::cerr << description << '\n' << Messages::get("env.invalid.action") << '\n';
        if (log_status == ReportStatus::failed)
            std::cerr << Messages::get("runtime.startup_failed.log_failed", {
                {"log_file", (log_directory / "kotavel.log").string()}}) << '\n';
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (const detail::ConsoleSettingsError& error) {
        const auto description = Messages::get("runtime.configuration_invalid.description",
            {{"key", error.key()}});
        const auto log_status = BootstrapLogger::configuration_invalid(
            log_directory, project_root.string(), error.key(), error.reason(), description);
        std::cerr << description << '\n'
                  << Messages::get("runtime.configuration_invalid.action", {{"reason", error.reason()}}) << '\n';
        if (log_status == ReportStatus::failed)
            std::cerr << Messages::get("runtime.startup_failed.log_failed", {
                {"log_file", (log_directory / "kotavel.log").string()}}) << '\n';
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (const std::exception& error) {
        const auto description = Messages::get("runtime.startup_failed.description");
        const auto log_file = log_directory / "kotavel.log";
        const auto log_status = BootstrapLogger::startup_failed(
            log_directory, project_root.string(), phase, error.what(), description);
        std::cerr << description << '\n'
                  << Messages::get("runtime.startup_failed.action", {{"log_file", log_file.string()}}) << '\n';
        if (log_status == ReportStatus::failed)
            std::cerr << Messages::get("runtime.startup_failed.log_failed", {{"log_file", log_file.string()}}) << '\n';
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (...) {
        const auto description = Messages::get("runtime.startup_failed.description");
        const auto log_file = log_directory / "kotavel.log";
        const auto log_status = BootstrapLogger::startup_failed(
            log_directory, project_root.string(), phase,
            "Unknown non-standard exception", description);
        std::cerr << description << '\n'
                  << Messages::get("runtime.startup_failed.action", {{"log_file", log_file.string()}}) << '\n';
        if (log_status == ReportStatus::failed)
            std::cerr << Messages::get("runtime.startup_failed.log_failed", {{"log_file", log_file.string()}}) << '\n';
        return 1;
    }
}
}
