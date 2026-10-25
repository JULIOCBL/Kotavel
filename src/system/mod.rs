//! Operating-system identity and platform selection.

mod network;
mod os;
mod paths;

pub use network::NativeNetworkAddressesProvider;
pub use os::{OperatingSystem, current_os};
pub(crate) use paths::packaged_resources_root;
pub use paths::{NativeRuntimePathResolver, NativeSystemPaths};
