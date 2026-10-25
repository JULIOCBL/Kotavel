//! Operating-system identity and platform selection.

mod os;

pub use os::{OperatingSystem, current_os};

// Compatibility exports for existing applications; new code should use the
// capability modules `network` and `system_paths` directly.
pub use crate::network::NativeNetworkAddressesProvider;
pub use crate::system_paths::{NativeRuntimePathResolver, NativeSystemPaths};
