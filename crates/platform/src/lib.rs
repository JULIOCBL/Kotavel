//! Native platform adapters grouped by capability and target operating system.
//!
//! The current migration re-exports adapters from `core` to preserve existing
//! applications. Their implementations are being moved into this crate without
//! changing the capability contracts.

pub mod filesystem;
pub mod network;
pub mod runtime_paths;
pub mod system_metrics;
pub mod system_paths;

pub use framework_core::platform_services::PlatformServices;
