#include <kotavel/console/runtime.hpp>
#include <kotavel/contracts/platform_services.hpp>
#include <kotavel/system/os.hpp>

#include <array>
#include <chrono>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

namespace {
struct TemporaryProject {
    std::filesystem::path root = std::filesystem::temp_directory_path() /
        ("kotavel-platform-services-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));

    TemporaryProject() {
        std::filesystem::create_directories(root);
        std::ofstream(root / ".env") << "APP_ENV=production\nAPP_DEBUG=false\n";
    }
    ~TemporaryProject() {
        std::error_code error;
        std::filesystem::remove_all(root, error);
    }
};

void require(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

std::filesystem::path environment_path(const char* key) {
    const char* value = std::getenv(key);
    return value && *value ? std::filesystem::path(value) : std::filesystem::path{};
}

std::filesystem::path expected_system_base(const char* variable, const char* fallback) {
    auto selected = environment_path(variable);
    if (!selected.empty() && selected.is_absolute()) return selected;
    auto home = environment_path(kotavel::OS::isWindows() ? "USERPROFILE" : "HOME");
    require(!home.empty() && home.is_absolute(), "A home directory must be available");
    return home / fallback;
}

void check_system_paths(const kotavel::contracts::SystemPaths& paths) {
    constexpr auto app = "KotavelPlatformTests";
    const auto config = paths.config_directory(app);
    const auto data = paths.data_directory(app);
    const auto cache = paths.cache_directory(app);

    require(config.is_absolute() && data.is_absolute() && cache.is_absolute(),
            "System paths must be absolute");
    require(config.filename() == app && data.filename() == app && cache.filename() == app,
            "System paths must end in the requested application directory");

    if (kotavel::OS::isMac()) {
        const auto home = environment_path("HOME");
        require(config == home / "Library" / "Preferences" / app,
                "macOS configuration path must use Library/Preferences");
        require(data == home / "Library" / "Application Support" / app,
                "macOS data path must use Library/Application Support");
        require(cache == home / "Library" / "Caches" / app,
                "macOS cache path must use Library/Caches");
    } else if (kotavel::OS::isLinux()) {
        require(config == expected_system_base("XDG_CONFIG_HOME", ".config") / app,
                "Linux configuration path must honor XDG_CONFIG_HOME");
        require(data == expected_system_base("XDG_DATA_HOME", ".local/share") / app,
                "Linux data path must honor XDG_DATA_HOME");
        require(cache == expected_system_base("XDG_CACHE_HOME", ".cache") / app,
                "Linux cache path must honor XDG_CACHE_HOME");
    } else {
        auto roaming = environment_path("APPDATA");
        if (roaming.empty()) roaming = environment_path("USERPROFILE") / "AppData" / "Roaming";
        auto local = environment_path("LOCALAPPDATA");
        if (local.empty()) local = environment_path("USERPROFILE") / "AppData" / "Local";
        require(config == roaming / app && data == roaming / app,
                "Windows configuration and data paths must use roaming app data");
        require(cache == local / app / "Cache",
                "Windows cache path must use local app data");
    }

    using PathResolver = std::filesystem::path (kotavel::contracts::SystemPaths::*)(
        std::string_view) const;
    constexpr std::array<std::string_view, 5> invalid_names = {
        std::string_view{}, ".", "..", "nested/name", "nested\\name"};
    constexpr std::array<PathResolver, 3> path_resolvers = {
        &kotavel::contracts::SystemPaths::config_directory,
        &kotavel::contracts::SystemPaths::data_directory,
        &kotavel::contracts::SystemPaths::cache_directory};
    for (const auto invalid : invalid_names) {
        for (const auto resolve : path_resolvers) {
            bool rejected = false;
            try {
                (paths.*resolve)(invalid);
            } catch (const std::invalid_argument&) {
                rejected = true;
            }
            require(rejected, "System paths must reject empty, traversal, and nested names");
        }
    }
}

void check_runtime_paths(const kotavel::contracts::RuntimePathResolver& resolver,
                         const std::filesystem::path& root) {
    std::filesystem::path executable;
    std::filesystem::path expected_root;
    std::filesystem::path expected_plugins;
    std::filesystem::path expected_translations;
    std::filesystem::path expected_certificates;

    if (kotavel::OS::isMac()) {
        const auto bundle = root / "Kotavel.app";
        executable = bundle / "Contents" / "MacOS" / "Kotavel";
        expected_root = bundle;
        expected_plugins = bundle / "Contents" / "PlugIns";
        expected_translations = bundle / "Contents" / "Resources" / "translations";
        expected_certificates = bundle / "Contents" / "Resources" / "certificates";
    } else if (kotavel::OS::isLinux()) {
        const auto prefix = root / "prefix";
        executable = prefix / "bin" / "kotavel";
        expected_root = prefix;
        expected_plugins = prefix / "lib" / "kotavel" / "plugins";
        expected_translations = prefix / "share" / "kotavel" / "translations";
        expected_certificates = prefix / "share" / "kotavel" / "certificates";
    } else {
        const auto directory = root / "application";
        executable = directory / "Kotavel.exe";
        expected_root = directory;
        expected_plugins = directory / "plugins";
        expected_translations = directory / "translations";
        expected_certificates = directory / "certificates";
    }

    const auto resolved = resolver.resolve(executable);
    require(resolved.application_root == expected_root && resolved.plugins == expected_plugins &&
                resolved.translations == expected_translations &&
                resolved.certificates == expected_certificates,
            "Runtime resource paths must match the active platform layout");

    bool rejected = false;
    try {
        (void)resolver.resolve({});
    } catch (const std::invalid_argument&) {
        rejected = true;
    }
    require(rejected, "Runtime path resolver must reject an empty executable path");
}
}

int main() {
    try {
        TemporaryProject project;
        const auto option = "--project-root=" + project.root.string();
        char program[] = "kotavel-platform-services-test";
        char* arguments[] = {program, const_cast<char*>(option.c_str())};
        std::string failure;

        const auto configure = [&](const kotavel::ConsoleContext& context) {
            require(context.root == std::filesystem::canonical(project.root),
                    "Console must canonicalize the consumer root");
            kotavel::ConsoleSettings settings;
            settings.logs.directory = project.root / "test-logs";
            settings.logs.console = false;
            return settings;
        };
        const auto execute = [&](kotavel::Application&, const kotavel::ConsoleContext& context) {
            try {
                require(context.container != nullptr, "Runtime must expose its service container");
                auto& container = *context.container;
                require(container.contains<kotavel::contracts::ProcessMetricsProvider>() &&
                            container.contains<kotavel::contracts::SystemPaths>() &&
                            container.contains<kotavel::contracts::RuntimePathResolver>() &&
                            container.contains<kotavel::contracts::PlatformServices>(),
                        "Platform bootstrap must register each platform contract");

                const auto& services = container.get<kotavel::contracts::PlatformServices>();
                const auto& metrics = container.get<kotavel::contracts::ProcessMetricsProvider>();
                const auto& paths = container.get<kotavel::contracts::SystemPaths>();
                const auto& runtime = container.get<kotavel::contracts::RuntimePathResolver>();
                require(&services.process_metrics == &metrics && &services.system_paths == &paths &&
                            &services.runtime_paths == &runtime,
                        "PlatformServices must reference the registered singleton contracts");
                require(&container.get<kotavel::contracts::SystemPaths>() == &paths,
                        "Platform adapters must be registered as singletons");

                const auto sample = metrics.read();
                require(sample.has_value() && sample->resident_memory_bytes > 0,
                        "Native process metrics provider must report resident memory");
                check_system_paths(paths);
                check_runtime_paths(runtime, project.root);
                return 0;
            } catch (const std::exception& error) {
                failure = error.what();
                return 1;
            }
        };

        const auto result = kotavel::run_console(2, arguments, configure, execute);
        if (!failure.empty()) throw std::runtime_error(failure);
        require(result == 0, "Runtime integration failed while booting platform services");
        std::cout << "Platform service bootstrap and path contracts: OK\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
