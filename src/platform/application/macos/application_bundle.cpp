#include "../../application_bundle.hpp"

#include <cstdlib>
#include <fstream>
#include <stdexcept>
#include <utility>

namespace kotavel::platform::detail {
std::optional<InstalledBundle> installed_bundle(const char* executable) {
    if (!executable || !*executable) return std::nullopt;
    auto binary = std::filesystem::path(executable);
    if (binary.is_relative()) binary = std::filesystem::absolute(binary);
    const auto macos = binary.parent_path();
    const auto contents = macos.parent_path();
    const auto bundle = contents.parent_path();
    if (macos.filename() != "MacOS" || contents.filename() != "Contents" ||
        bundle.extension() != ".app")
        return std::nullopt;

    const auto resources = contents / "Resources";
    if (!std::filesystem::is_directory(resources) ||
        !std::filesystem::is_regular_file(resources / "kotavel-environment"))
        return std::nullopt;

    std::string environment = "production";
    std::ifstream selection(resources / "kotavel-environment", std::ios::binary);
    if (selection) {
        std::getline(selection, environment);
        while (!environment.empty() && (environment.back() == '\r' || environment.back() == ' ' ||
                                         environment.back() == '\t'))
            environment.pop_back();
        const auto first = environment.find_first_not_of(" \t");
        if (first != std::string::npos) environment.erase(0, first);
        if (environment != "production" && environment != "local" && environment != "development")
            throw std::runtime_error("Application bundle has an invalid kotavel-environment value");
    }

    const char* home = std::getenv("HOME");
    if (!home || !*home) throw std::runtime_error("HOME is unavailable for the application data directory");
    const auto data = std::filesystem::path(home) / "Library" / "Application Support" / bundle.stem();
    std::filesystem::create_directories(data);
    const auto filename = environment == "production" ? ".env" : ".env." + environment;
    const auto bundled_environment = resources / filename;
    if (!std::filesystem::is_regular_file(bundled_environment))
        throw std::runtime_error("Application bundle is missing " + filename);
    const auto user_environment = data / filename;
    if (!std::filesystem::exists(user_environment))
        std::filesystem::copy_file(bundled_environment, user_environment);
    return InstalledBundle{resources, data, std::move(environment)};
}
} // namespace kotavel::platform::detail
