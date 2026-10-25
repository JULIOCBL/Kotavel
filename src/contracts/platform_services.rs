use super::{ProcessMetricsProvider, RuntimePathResolver, SystemPaths};
use std::sync::Arc;

/// Shared native services registered by the application container.
#[derive(Clone)]
pub struct PlatformServices {
    pub process_metrics: Arc<dyn ProcessMetricsProvider>,
    pub system_paths: Arc<dyn SystemPaths>,
    pub runtime_paths: Arc<dyn RuntimePathResolver>,
}
