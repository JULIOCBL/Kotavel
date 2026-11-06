//! Network-facing platform capability contracts.

mod native;
mod network_addresses;

/// Common local HTTP ports that Kotavel discovery may probe for framework
/// applications. This list is only a candidate set; it does not start a scan.
pub const DISCOVERY_PORT_CANDIDATES: &[u16] = &[
    3000, 5000, 8000, 8080, 9000, 28474, 28475, 28476, 28477, 28478,
];

pub use native::NativeNetworkAddressesProvider;
pub use network_addresses::NetworkAddressesProvider;

#[cfg(test)]
#[path = "../../tests/unit/network/ports.rs"]
mod tests;
