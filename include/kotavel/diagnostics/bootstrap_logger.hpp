#pragma once

#include <kotavel/diagnostics/logger.hpp>

namespace kotavel {

// Logger temporal para mensajes de arranque ocurridos antes de Application::boot().
class BootstrapLogger final {
public:
    [[nodiscard]] static ReportStatus write(const std::filesystem::path& directory,
                                             const LogRecord& record) noexcept;
    [[nodiscard]] static ReportStatus environment_file_open_failed(
        const std::filesystem::path& directory, std::string_view file,
        std::string_view example, std::string_view project_root,
        std::string_view reason, std::string_view message) noexcept;
    [[nodiscard]] static ReportStatus environment_file_invalid(
        const std::filesystem::path& directory, std::string_view file,
        std::string_view project_root, std::size_t line,
        std::string_view reason, std::string_view message) noexcept;
    [[nodiscard]] static ReportStatus configuration_invalid(
        const std::filesystem::path& directory, std::string_view project_root,
        std::string_view key, std::string_view reason,
        std::string_view message) noexcept;
    [[nodiscard]] static ReportStatus startup_failed(
        const std::filesystem::path& directory, std::string_view project_root,
        std::string_view phase, std::string_view reason,
        std::string_view message) noexcept;
};

} // namespace kotavel
