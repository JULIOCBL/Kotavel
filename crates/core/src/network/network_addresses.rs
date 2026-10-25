use std::io;

/// Provides IPv4 addresses assigned to active, non-loopback interfaces.
pub trait NetworkAddressesProvider: Send + Sync {
    fn local_ipv4_addresses(&self) -> io::Result<Vec<String>>;
}
