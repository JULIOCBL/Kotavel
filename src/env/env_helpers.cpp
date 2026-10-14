#include <kotavel/env/env.hpp>
#include "environment_internal.hpp"

namespace kotavel {
// Los helpers y su ámbito comparten una fuente por hilo, restaurada mediante RAII.
namespace { thread_local const Env* active_env = nullptr; }
const Env& detail::current_environment() {
    if (active_env) return *active_env;
    static const ProcessEnvironment source;
    static const Env reader(source);
    return reader;
}

EnvScope::EnvScope(const Env& reader) noexcept : previous_(active_env) { active_env = &reader; }
EnvScope::~EnvScope() { active_env = previous_; }
std::string env(std::string_view key, std::string_view fallback) {
    return detail::current_environment().string(key, std::string(fallback));
}
std::string env(std::string_view key, const char* fallback) {
    if (!fallback) throw std::invalid_argument("Environment string fallback cannot be null");
    return env(key, std::string_view(fallback));
}
bool env(std::string_view key, bool fallback) { return detail::current_environment().boolean(key, fallback); }
std::string env(std::string_view key, const std::string& fallback) {
    return env(key, std::string_view(fallback));
}
EnvironmentValue env(std::string_view key) { return EnvironmentValue(detail::current_environment().get(key)); }
} // namespace kotavel
