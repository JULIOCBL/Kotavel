#include "platform_services.hpp"

namespace kotavel::platform::detail {
namespace {
class PlatformServiceProvider final : public ServiceProvider {
public:
    void register_services(Container& services) override {
        // La aplicación puede registrar su proveedor antes; el adaptador nativo es fallback.
        if (!services.contains<contracts::ProcessMetricsProvider>()) {
            services.singleton<contracts::ProcessMetricsProvider>([](Container&) {
                return make_process_metrics_provider();
            });
        }
        if (!services.contains<contracts::SystemPaths>()) {
            services.singleton<contracts::SystemPaths>([](Container&) {
                return make_system_paths_provider();
            });
        }
        if (!services.contains<contracts::RuntimePathResolver>()) {
            services.singleton<contracts::RuntimePathResolver>([](Container&) {
                return make_runtime_path_resolver();
            });
        }
        if (!services.contains<contracts::NetworkAddressesProvider>()) {
            services.singleton<contracts::NetworkAddressesProvider>([](Container&) {
                return make_network_addresses_provider();
            });
        }
        if (!services.contains<contracts::PlatformServices>()) {
            services.singleton<contracts::PlatformServices>([](Container& container) {
                return std::make_unique<contracts::PlatformServices>(contracts::PlatformServices{
                    container.get<contracts::ProcessMetricsProvider>(),
                    container.get<contracts::SystemPaths>(),
                    container.get<contracts::RuntimePathResolver>()});
            });
        }
    }
};
}

std::unique_ptr<ServiceProvider> make_platform_service_provider() {
    return std::make_unique<PlatformServiceProvider>();
}
} // namespace kotavel::platform::detail
