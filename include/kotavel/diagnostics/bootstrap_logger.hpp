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
};

} // namespace kotavel
