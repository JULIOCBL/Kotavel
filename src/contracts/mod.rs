//! Ports shared between the framework core and platform-specific adapters.
//!
//! Implementations live in the owning modules or platform crates; core code
//! depends on these contracts instead of operating-system APIs.

mod network_addresses;
mod platform_services;
mod runtime_paths;
mod session_identifier_transport;
mod system_paths;

pub use crate::filesystem::{FileCursor, FileLock, Filesystem};
pub use crate::metrics::{ProcessMetricsProvider, ProcessMetricsSnapshot};
pub use crate::session::SessionStore;
pub use network_addresses::NetworkAddressesProvider;
pub use platform_services::PlatformServices;
pub use runtime_paths::{RuntimePathResolver, RuntimePaths};
pub use session_identifier_transport::SessionIdentifierTransport;
pub use system_paths::SystemPaths;

#[cfg(test)]
mod tests;
