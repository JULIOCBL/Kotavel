#include <kotavel/env/env.hpp>
#include "environment_internal.hpp"
#include <fstream>

namespace kotavel {
// Carga el archivo seleccionado y aplica la precedencia del entorno del proceso.
FileEnvironment::FileEnvironment(const std::filesystem::path& root, std::string_view environment) {
    if (environment != "production" && environment != "local" && environment != "development")
        throw std::invalid_argument("Selected environment must be local, development or production");
    const auto path = root / (environment == "production" ? ".env" : ".env." + std::string(environment));
    std::ifstream file(path, std::ios::binary);
    if (!file) throw detail::EnvironmentFileOpenError(path);
    // Acotar también el archivo antes de reservar memoria o analizar sus líneas.
    std::string content;
    char c;
    while (file.get(c)) {
        if (content.size() == 65536) throw std::invalid_argument("Environment file exceeds 64 KiB");
        if (c == '\0') throw std::invalid_argument("Environment file contains a null byte");
        content += c;
    }
    if (!file.eof()) throw std::runtime_error("Cannot read environment file");
    auto trim = [](std::string_view text) {
        const auto first = text.find_first_not_of(" \t\r");
        if (first == text.npos) return std::string_view{};
        return text.substr(first, text.find_last_not_of(" \t\r") - first + 1);
    };
    std::string_view remaining(content);
    while (!remaining.empty()) {
        auto end = remaining.find('\n');
        auto line = trim(remaining.substr(0, end));
        remaining = end == remaining.npos ? std::string_view{} : remaining.substr(end + 1);
        if (line.empty() || line.front() == '#') continue;
        const auto equal = line.find('=');
        if (equal == line.npos) throw std::invalid_argument("Environment file requires KEY=VALUE");
        const auto key = trim(line.substr(0, equal));
        detail::validate_key(key);
        if (key.find_first_not_of("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_") != key.npos ||
            (key.front() >= '0' && key.front() <= '9'))
            throw std::invalid_argument("Invalid environment file key");
        auto value = trim(line.substr(equal + 1));
        if (!value.empty() && (value.front() == '\'' || value.front() == '"')) {
            const auto closing = value.find(value.front(), 1);
            if (closing == value.npos) throw std::invalid_argument("Unclosed environment quote");
            auto tail = trim(value.substr(closing + 1));
            if (!tail.empty() && tail.front() != '#') throw std::invalid_argument("Invalid environment quote suffix");
            value = value.substr(0, closing + 1);
        } else {
            for (std::size_t i = 0; i < value.size(); ++i)
                if (value[i] == '#' && (i == 0 || value[i - 1] == ' ' || value[i - 1] == '\t')) {
                    value = trim(value.substr(0, i)); break;
                }
        }
        if (value.size() > 4096) throw std::invalid_argument("Environment value exceeds 4096 bytes");
        if (!values_.emplace(std::string(key), std::string(value)).second)
            throw std::invalid_argument("Duplicate environment file key: " + std::string(key));
    }
    // Publicar también la selección cuando el archivo no declara APP_ENV.
    values_.try_emplace("APP_ENV", std::string(environment));
    const Env reader(*this);
    if (reader.string("APP_ENV", std::string(environment)) != environment)
        throw std::invalid_argument("APP_ENV does not match selected environment");
}
std::optional<std::string> FileEnvironment::read(std::string_view key) const {
    if (auto value = process_.read(key)) return value;
    const auto found = values_.find(key);
    if (found == values_.end()) return std::nullopt;
    return found->second;
}
} // namespace kotavel
