#pragma once

#include <string>
#include <vector>

namespace kotavel::contracts {

// Provides IPv4 addresses assigned to active, non-loopback network interfaces.
class NetworkAddressesProvider {
public:
    virtual ~NetworkAddressesProvider() = default;
    [[nodiscard]] virtual std::vector<std::string> local_ipv4_addresses() const = 0;
};

} // namespace kotavel::contracts
