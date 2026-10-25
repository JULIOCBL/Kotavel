//! Ports shared between the framework core and platform-specific adapters.
//!
//! Implementations live in the owning modules or platform crates; core code
//! depends on these contracts instead of operating-system APIs.

//! Compatibility facade for capability contracts.
//!
//! New code should import contracts from their capability modules, such as
//! [`crate::system_paths`] and [`crate::runtime_paths`].

pub use crate::filesystem::{FileCursor, FileLock, Filesystem};
pub use crate::metrics::{ProcessMetricsProvider, ProcessMetricsSnapshot};
pub use crate::network::NetworkAddressesProvider;
pub use crate::platform_services::PlatformServices;
pub use crate::runtime_paths::{RuntimePathResolver, RuntimePaths};
pub use crate::session::SessionIdentifierTransportContract as SessionIdentifierTransport;
pub use crate::session::SessionStore;
pub use crate::system_paths::SystemPaths;

#[cfg(test)]
mod tests;
