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
    /// `None` tries the framework's ordered free-port candidates.
    pub port: Option<u16>,
    /// Zero lets the transport choose its default worker count.
    pub threads: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: None,
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
        if self.port == Some(0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "HTTP_PORT must be between 1 and 65535 when explicitly configured",
            ));
        }
        Ok(())
    }

    /// Resolves IP-literal hosts without restricting adapters that support DNS names.
    pub fn socket_address(&self) -> io::Result<Option<SocketAddr>> {
        self.validate()?;
        Ok(self.port.and_then(|port| {
            self.host
                .parse::<IpAddr>()
                .ok()
                .map(|host| SocketAddr::new(host, port))
        }))
    }

    /// Opens the requested listener, or tries the framework's ordered ports
    /// when `port` is `None`. Binding is the availability check, avoiding a
    /// separate probe that could race another process.
    pub fn bind_listener(&self) -> io::Result<std::net::TcpListener> {
        self.validate()?;
        if let Some(port) = self.port {
            return std::net::TcpListener::bind((self.host.as_str(), port));
        }

        bind_first_available(&self.host, crate::network::DISCOVERY_PORT_CANDIDATES)
    }
}

fn bind_first_available(host: &str, candidates: &[u16]) -> io::Result<std::net::TcpListener> {
    let mut last_addr_in_use = None;
    for port in candidates {
        match std::net::TcpListener::bind((host, *port)) {
            Ok(listener) => return Ok(listener),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                last_addr_in_use = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_addr_in_use.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "DISCOVERY_PORT_CANDIDATES is empty",
        )
    }))
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
#[path = "../../tests/unit/http/server.rs"]
mod tests;
