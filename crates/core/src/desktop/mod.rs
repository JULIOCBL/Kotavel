//! Transport-independent desktop routing and window lifecycle contracts.
//!
//! Platform integrations implement [`DesktopPlatform`] and [`WindowManager`],
//! leaving route policy and deep-link validation portable across operating
//! systems. The Qt/QML adapter is a separate migration step.

mod context;
mod kernel;
#[cfg(feature = "desktop-qt")]
mod qt;
mod router;

pub use context::{
    DesktopContext, DesktopError, DispatchResult, View, WindowManager, WindowSettings, view,
};
pub use kernel::{DesktopKernel, DesktopPlatform, dispatch_deep_link};
#[cfg(feature = "desktop-qt")]
pub use qt::QtDesktopPlatform;
pub use router::{DesktopRouter, Route};

pub(crate) use context::{Handler, valid_route, valid_segmented_name};

#[cfg(test)]
mod tests;
