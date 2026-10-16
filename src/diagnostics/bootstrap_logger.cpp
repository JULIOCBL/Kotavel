#include <kotavel/diagnostics/bootstrap_logger.hpp>

#include <array>
#include <utility>

namespace kotavel {

ReportStatus BootstrapLogger::write(const std::filesystem::path& directory,
                                    const LogRecord& record) noexcept {
    try {
        LogOptions options;
        options.directory = directory;
        options.module = record.module.empty() ? "bootstrap" : std::string(record.module);
        options.min_level = LogLevel::trace;
        options.console = false;
        Logger logger(std::move(options));
        return logger.write(record);
    } catch (...) {
        return ReportStatus::failed;
    }
}

ReportStatus BootstrapLogger::environment_file_open_failed(
    const std::filesystem::path& directory, std::string_view file,
    std::string_view example, std::string_view project_root,
    std::string_view reason, std::string_view message) noexcept {
    try {
        const std::array<ContextField, 4> fields{{
            {"file", std::string(file)},
            {"example", std::string(example)},
            {"project_root", std::string(project_root)},
            {"reason", std::string(reason)}
        }};
        const LogRecord record{LogLevel::error, "environment.file_open_failed",
            "load_environment", message, "bootstrap", fields};
        return write(directory, record);
    } catch (...) {
        return ReportStatus::failed;
    }
}

} // namespace kotavel
