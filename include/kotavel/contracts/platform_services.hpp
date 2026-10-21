#pragma once

#include <kotavel/contracts/process_metrics_provider.hpp>
#include <kotavel/contracts/runtime_paths.hpp>
#include <kotavel/contracts/system_paths.hpp>

namespace kotavel::contracts {

// Vista agrupada de las capacidades nativas registradas para la aplicación.
// Las referencias pertenecen al Container y vencen cuando este se cierra.
struct PlatformServices final {
    ProcessMetricsProvider& process_metrics;
    SystemPaths& system_paths;
    RuntimePathResolver& runtime_paths;
};

} // namespace kotavel::contracts
