//! Replaceable HTTP transport boundary.

use super::HttpKernel;
use std::{
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
};

pub type ServerError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type ServerStartedCallback = Arc<dyn Fn() + Send + Sync + 'static>;
pub type ServerFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ServerError>> + Send + 'a>>;

/// Listener configuration shared by concrete HTTP server adapters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSettings {
    /// Defaults to all local interfaces so remote clients can connect.
    pub host: String,
    pub port: u16,
    /// Zero lets the transport choose its default worker count.
    pub threads: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
            threads: 0,
        }
    }
}

impl ServerSettings {
    pub fn validate(&self) -> io::Result<()> {
        if self.host.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTP bind host cannot be empty",
            ));
        }
        if self.port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTP port must be between 1 and 65535",
            ));
        }
        Ok(())
    }

    /// Resolves IP-literal hosts without restricting adapters that support DNS names.
    pub fn socket_address(&self) -> io::Result<Option<SocketAddr>> {
        self.validate()?;
        Ok(self
            .host
            .parse::<IpAddr>()
            .ok()
            .map(|host| SocketAddr::new(host, self.port)))
    }
}

/// Transport port. Implementations live outside the kernel so the framework
/// does not depend on Axum, Tokio, or a platform-specific listener.
pub trait Server: Send + Sync {
    fn run<'a>(
        &'a self,
        kernel: Arc<HttpKernel>,
        settings: ServerSettings,
        started: Option<ServerStartedCallback>,
    ) -> ServerFuture<'a>;

    fn stop(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpKernel;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn defaults_match_the_framework_listener_contract() {
        let settings = ServerSettings::default();
        assert_eq!(settings.host, "0.0.0.0");
        assert_eq!(settings.port, 8080);
        assert_eq!(settings.threads, 0);
        assert_eq!(
            settings.socket_address().unwrap(),
            Some("0.0.0.0:8080".parse().unwrap())
        );
    }

    #[test]
    fn rejects_invalid_listener_settings_and_preserves_dns_hosts() {
        let mut settings = ServerSettings::default();
        settings.host.clear();
        assert_eq!(
            settings.validate().unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        settings.host = "localhost".into();
        settings.port = 0;
        assert!(settings.validate().is_err());
        settings.port = 8080;
        assert_eq!(settings.socket_address().unwrap(), None);
    }

    struct FakeServer {
        runs: AtomicUsize,
        stopped: AtomicBool,
    }

    impl Server for FakeServer {
        fn run<'a>(
            &'a self,
            kernel: Arc<HttpKernel>,
            settings: ServerSettings,
            started: Option<ServerStartedCallback>,
        ) -> ServerFuture<'a> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                assert!(kernel.started());
                assert!(kernel.routes().sealed());
                assert_eq!(settings.port, 8080);
                if let Some(started) = started {
                    started();
                }
                Ok(())
            })
        }

        fn stop(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn kernel_seals_once_delegates_start_and_forwards_stop() {
        let server = FakeServer {
            runs: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
        };
        let started_count = Arc::new(AtomicUsize::new(0));
        let callback_count = Arc::clone(&started_count);
        let callback: ServerStartedCallback = Arc::new(move || {
            callback_count.fetch_add(1, Ordering::SeqCst);
        });
        let mut kernel = HttpKernel::new();

        block_on(kernel.run(&server, ServerSettings::default(), Some(callback))).unwrap();
        assert!(kernel.started());
        assert_eq!(server.runs.load(Ordering::SeqCst), 1);
        assert_eq!(started_count.load(Ordering::SeqCst), 1);
        assert!(block_on(kernel.run(&server, ServerSettings::default(), None)).is_err());
        assert_eq!(server.runs.load(Ordering::SeqCst), 1);

        kernel.stop(&server);
        assert!(server.stopped.load(Ordering::SeqCst));
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};

        let mut context = Context::from_waker(Waker::noop());
        let mut future = Box::pin(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("fake server future unexpectedly suspended"),
        }
    }
}
