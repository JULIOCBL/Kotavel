#include "../../platform_services.hpp"

#include <arpa/inet.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <sys/socket.h>

#include <algorithm>
#include <array>
#include <memory>
#include <string>
#include <vector>

namespace kotavel::platform::detail {
namespace {
class PosixNetworkAddressesProvider final : public contracts::NetworkAddressesProvider {
public:
    std::vector<std::string> local_ipv4_addresses() const override {
        ifaddrs* raw_interfaces = nullptr;
        if (getifaddrs(&raw_interfaces) != 0) return {};
        const std::unique_ptr<ifaddrs, decltype(&freeifaddrs)> interfaces(raw_interfaces, freeifaddrs);

        std::vector<std::string> addresses;
        for (const auto* interface = interfaces.get(); interface; interface = interface->ifa_next) {
            if (!interface->ifa_addr || (interface->ifa_flags & IFF_UP) == 0 ||
                (interface->ifa_flags & IFF_LOOPBACK) != 0 || interface->ifa_addr->sa_family != AF_INET)
                continue;

            const auto& address = reinterpret_cast<const sockaddr_in*>(interface->ifa_addr)->sin_addr;
            std::array<char, INET_ADDRSTRLEN> text{};
            if (!inet_ntop(AF_INET, &address, text.data(), static_cast<socklen_t>(text.size()))) continue;
            const std::string value(text.data());
            if (value.starts_with("169.254.")) continue;
            if (std::find(addresses.begin(), addresses.end(), value) == addresses.end())
                addresses.push_back(value);
        }
        return addresses;
    }
};
}

std::unique_ptr<contracts::NetworkAddressesProvider> make_network_addresses_provider() {
    return std::make_unique<PosixNetworkAddressesProvider>();
}
} // namespace kotavel::platform::detail
