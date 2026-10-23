#include "../../platform_services.hpp"

#define NOMINMAX
#include <winsock2.h>
#include <windows.h>
#include <iphlpapi.h>
#include <ws2tcpip.h>

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

namespace kotavel::platform::detail {
namespace {
class WindowsNetworkAddressesProvider final : public contracts::NetworkAddressesProvider {
public:
    std::vector<std::string> local_ipv4_addresses() const override {
        ULONG buffer_size = 16 * 1024;
        auto buffer = std::make_unique<BYTE[]>(buffer_size);
        auto* adapters = reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buffer.get());
        ULONG status = GetAdaptersAddresses(AF_INET,
            GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
            nullptr, adapters, &buffer_size);
        if (status == ERROR_BUFFER_OVERFLOW) {
            buffer = std::make_unique<BYTE[]>(buffer_size);
            adapters = reinterpret_cast<IP_ADAPTER_ADDRESSES*>(buffer.get());
            status = GetAdaptersAddresses(AF_INET,
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                nullptr, adapters, &buffer_size);
        }
        if (status != NO_ERROR) return {};

        std::vector<std::string> addresses;
        for (const auto* adapter = adapters; adapter; adapter = adapter->Next) {
            if (adapter->OperStatus != IfOperStatusUp) continue;
            for (const auto* unicast = adapter->FirstUnicastAddress; unicast; unicast = unicast->Next) {
                if (!unicast->Address.lpSockaddr || unicast->Address.lpSockaddr->sa_family != AF_INET)
                    continue;
                const auto& address = reinterpret_cast<const sockaddr_in*>(unicast->Address.lpSockaddr)->sin_addr;
                char text[INET_ADDRSTRLEN]{};
                if (!InetNtopA(AF_INET, &address, text, sizeof(text))) continue;
                const std::string value(text);
                if (value == "0.0.0.0" || value.starts_with("127.") || value.starts_with("169.254."))
                    continue;
                if (std::find(addresses.begin(), addresses.end(), value) == addresses.end())
                    addresses.push_back(value);
            }
        }
        return addresses;
    }
};
}

std::unique_ptr<contracts::NetworkAddressesProvider> make_network_addresses_provider() {
    return std::make_unique<WindowsNetworkAddressesProvider>();
}
} // namespace kotavel::platform::detail
