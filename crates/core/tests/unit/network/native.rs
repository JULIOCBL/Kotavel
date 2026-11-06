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
