//! Network adapters grouped by target operating system.

pub use framework_core::network::{NativeNetworkAddressesProvider, NetworkAddressesProvider};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
