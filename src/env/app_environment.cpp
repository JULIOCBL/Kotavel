#include <kotavel/env/env.hpp>

#include "environment_internal.hpp"

#include <algorithm>

namespace kotavel {
// Valida la identidad de la aplicación antes de iniciar sus operaciones.
AppEnvironment AppEnvironment::load(const Env& env) {
    AppEnvironment result{env.string("APP_NAME", "Kotavel"),
                          env.string("APP_ENV", "production"), env.boolean("APP_DEBUG", false)};
    if (result.name.empty() || result.name.size() > 64 ||
        std::any_of(result.name.begin(), result.name.end(), [](unsigned char c) { return c < 32 || c == 127; }))
        throw std::invalid_argument("APP_NAME requires 1 to 64 bytes without control characters");
    if (result.environment != "local" && result.environment != "development" && result.environment != "production" &&
        result.environment != "testing" && result.environment != "staging")
        throw std::invalid_argument("APP_ENV must be local, development, production, testing or staging");
    return result;
}
AppEnvironment AppEnvironment::load() { return load(detail::current_environment()); }

} // namespace kotavel
