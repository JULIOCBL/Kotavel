#pragma once

#include <memory>

#include <kotavel/contracts/process_metrics_provider.hpp>
#include <kotavel/contracts/platform_services.hpp>
#include <kotavel/contracts/network_addresses_provider.hpp>
#include <kotavel/contracts/runtime_paths.hpp>
#include <kotavel/contracts/system_paths.hpp>
#include <kotavel/container/container.hpp>

namespace kotavel::platform::detail {

[[nodiscard]] std::unique_ptr<contracts::ProcessMetricsProvider>
make_process_metrics_provider();

[[nodiscard]] std::unique_ptr<contracts::SystemPaths> make_system_paths_provider();

[[nodiscard]] std::unique_ptr<contracts::RuntimePathResolver>
make_runtime_path_resolver();

[[nodiscard]] std::unique_ptr<contracts::NetworkAddressesProvider>
make_network_addresses_provider();

void configure_console_output() noexcept;

[[nodiscard]] std::unique_ptr<ServiceProvider> make_platform_service_provider();

} // namespace kotavel::platform::detail
