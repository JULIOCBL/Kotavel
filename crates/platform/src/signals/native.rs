use framework_core::contracts::ShutdownSignal;
use std::{future::Future, io, pin::Pin};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeShutdownSignal;

impl ShutdownSignal for NativeShutdownSignal {
    fn wait(&self) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>> {
        Box::pin(async move {
            #[cfg(unix)]
            {
                let mut terminate =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
                tokio::select! {
                    result = tokio::signal::ctrl_c() => result,
                    _ = terminate.recv() => Ok(()),
                }
            }

            #[cfg(windows)]
            {
                let mut ctrl_break = tokio::signal::windows::ctrl_break()?;
                tokio::select! {
                    result = tokio::signal::ctrl_c() => result,
                    _ = ctrl_break.recv() => Ok(()),
                }
            }

            #[cfg(not(any(unix, windows)))]
            {
                tokio::signal::ctrl_c().await
            }
        })
    }
}
