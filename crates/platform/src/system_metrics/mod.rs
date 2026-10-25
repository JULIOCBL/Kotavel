//! Native process measurement adapter.
pub use framework_core::metrics::{
    NativeProcessMetricsProvider, ProcessMetricsProvider, ProcessMetricsSnapshot,
};

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod windows;
