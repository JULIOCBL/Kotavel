//! Network-facing platform capability contracts.

mod native;
mod network_addresses;

pub use native::NativeNetworkAddressesProvider;
pub use network_addresses::NetworkAddressesProvider;
