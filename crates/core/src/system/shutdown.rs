use std::{future::Future, io, pin::Pin};

/// Contract for waiting until the host requests a graceful application shutdown.
pub trait ShutdownSignal: Send + Sync {
    fn wait(&self) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>>;
}
