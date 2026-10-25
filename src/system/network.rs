//! Native adapter for local IPv4 interface addresses.

use crate::contracts::NetworkAddressesProvider;
use std::{collections::HashSet, io, net::Ipv4Addr};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeNetworkAddressesProvider;

impl NetworkAddressesProvider for NativeNetworkAddressesProvider {
    fn local_ipv4_addresses(&self) -> io::Result<Vec<String>> {
        platform::local_ipv4_addresses()
    }
}

fn append_address(address: Ipv4Addr, addresses: &mut Vec<String>, seen: &mut HashSet<Ipv4Addr>) {
    if address.is_unspecified() || address.is_loopback() || address.is_link_local() {
        return;
    }
    if seen.insert(address) {
        addresses.push(address.to_string());
    }
}

#[cfg(unix)]
mod platform {
    use super::{HashSet, Ipv4Addr, append_address, io};
    use std::ptr;

    struct InterfaceList(*mut libc::ifaddrs);

    impl Drop for InterfaceList {
        fn drop(&mut self) {
            // SAFETY: the pointer comes from getifaddrs and is released exactly once.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }

    pub(super) fn local_ipv4_addresses() -> io::Result<Vec<String>> {
        let mut head = ptr::null_mut();
        // SAFETY: `head` is a valid output pointer; ownership is held by InterfaceList.
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let _interfaces = InterfaceList(head);
        let mut addresses = Vec::new();
        let mut seen = HashSet::new();
        let mut current = head;
        // SAFETY: getifaddrs returns a valid linked list until freeifaddrs.
        unsafe {
            while !current.is_null() {
                let interface = &*current;
                if !interface.ifa_addr.is_null()
                    && interface.ifa_flags as i32 & libc::IFF_UP != 0
                    && interface.ifa_flags as i32 & libc::IFF_LOOPBACK == 0
                    && (*interface.ifa_addr).sa_family as i32 == libc::AF_INET
                {
                    let socket = &*(interface.ifa_addr.cast::<libc::sockaddr_in>());
                    let address = Ipv4Addr::from(socket.sin_addr.s_addr.to_ne_bytes());
                    append_address(address, &mut addresses, &mut seen);
                }
                current = interface.ifa_next;
            }
        }
        Ok(addresses)
    }
}

#[cfg(windows)]
mod platform {
    use super::{HashSet, Ipv4Addr, append_address, io};
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::ERROR_BUFFER_OVERFLOW,
        NetworkManagement::{
            IpHelper::{
                GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
                GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
            },
            Ndis::NET_IF_OPER_STATUS_UP,
        },
        Networking::WinSock::{AF_INET, SOCKADDR_IN},
    };

    const INITIAL_BUFFER_BYTES: usize = 16 * 1024;
    const FLAGS: u32 = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    pub(super) fn local_ipv4_addresses() -> io::Result<Vec<String>> {
        let mut buffer = vec![0usize; INITIAL_BUFFER_BYTES.div_ceil(size_of::<usize>())];
        let mut buffer_bytes = (buffer.len() * size_of::<usize>()) as u32;
        // SAFETY: the aligned buffer is sized as requested and all pointers are valid.
        let mut result = unsafe {
            GetAdaptersAddresses(
                AF_INET as u32,
                FLAGS,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut buffer_bytes,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW {
            buffer = vec![0usize; (buffer_bytes as usize).div_ceil(size_of::<usize>())];
            // SAFETY: retry uses an aligned buffer with the size returned by the API.
            result = unsafe {
                GetAdaptersAddresses(
                    AF_INET as u32,
                    FLAGS,
                    std::ptr::null(),
                    buffer.as_mut_ptr().cast(),
                    &mut buffer_bytes,
                )
            };
        }
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }

        let mut addresses = Vec::new();
        let mut seen = HashSet::new();
        let mut adapter = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: on success the buffer contains the linked adapter/unicast lists.
        unsafe {
            while !adapter.is_null() {
                let current = &*adapter;
                if current.OperStatus == NET_IF_OPER_STATUS_UP {
                    let mut unicast = current.FirstUnicastAddress;
                    while !unicast.is_null() {
                        let socket = (*unicast).Address;
                        if !socket.lpSockaddr.is_null()
                            && socket.iSockaddrLength as usize >= size_of::<SOCKADDR_IN>()
                            && (*socket.lpSockaddr).sa_family == AF_INET
                        {
                            let ipv4 = &*socket.lpSockaddr.cast::<SOCKADDR_IN>();
                            let address = Ipv4Addr::from(ipv4.sin_addr.S_un.S_addr.to_ne_bytes());
                            append_address(address, &mut addresses, &mut seen);
                        }
                        unicast = (*unicast).Next;
                    }
                }
                adapter = current.Next;
            }
        }
        Ok(addresses)
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::io;

    pub(super) fn local_ipv4_addresses() -> io::Result<Vec<String>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Native network address enumeration is unsupported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_filter_excludes_non_routable_addresses_and_deduplicates() {
        let mut addresses = Vec::new();
        let mut seen = HashSet::new();
        for address in [
            Ipv4Addr::new(192, 168, 1, 10),
            Ipv4Addr::new(192, 168, 1, 10),
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::new(169, 254, 20, 1),
            Ipv4Addr::UNSPECIFIED,
        ] {
            append_address(address, &mut addresses, &mut seen);
        }
        assert_eq!(addresses, ["192.168.1.10"]);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn native_provider_returns_unique_usable_ipv4_addresses() {
        let addresses = NativeNetworkAddressesProvider
            .local_ipv4_addresses()
            .unwrap();
        let mut seen = HashSet::new();
        for address in addresses {
            let parsed: Ipv4Addr = address.parse().unwrap();
            assert!(!parsed.is_unspecified() && !parsed.is_loopback() && !parsed.is_link_local());
            assert!(seen.insert(parsed));
        }
    }
}
