#pragma once
#include <kotavel/env/env.hpp>
#include <stdexcept>
#include <utility>

namespace kotavel::detail {
class EnvironmentFileOpenError final : public std::runtime_error {
public:
    explicit EnvironmentFileOpenError(std::filesystem::path path)
        : std::runtime_error("Cannot open environment file: " + path.string()),
          path_(std::move(path)) {}

    [[nodiscard]] const std::filesystem::path& path() const noexcept { return path_; }

private:
    std::filesystem::path path_;
};

class EnvironmentFileInvalidError final : public std::invalid_argument {
public:
    EnvironmentFileInvalidError(std::filesystem::path path, std::size_t line,
                                std::string reason)
        : std::invalid_argument(std::move(reason)), path_(std::move(path)), line_(line) {}

    [[nodiscard]] const std::filesystem::path& path() const noexcept { return path_; }
    [[nodiscard]] std::size_t line() const noexcept { return line_; }

private:
    std::filesystem::path path_;
    std::size_t line_;
};

// Validación compartida; detalle privado, no forma parte de la API instalada.
inline void validate_key(std::string_view key) {
    if (key.empty() || key.size() > 256 || key.find_first_of("=\0", 0, 2) != key.npos)
        throw std::invalid_argument("Invalid environment key");
}

// Lector activo del hilo o lector predeterminado del proceso.
const Env& current_environment();
}
