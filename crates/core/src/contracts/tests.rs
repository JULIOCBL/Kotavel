use super::*;

#[test]
fn session_identifier_policy_is_usable_through_its_port() {
    let transport: Box<dyn SessionIdentifierTransport> =
        Box::new(crate::session::SessionIdentifierTransport::header("X-Kotavel-Session").unwrap());
    let id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(transport.read(None, Some(id)).as_deref(), Some(id));
    assert_eq!(
        transport.response_header(id).unwrap(),
        ("X-Kotavel-Session".into(), id.into())
    );
}

#[test]
fn runtime_paths_are_empty_and_owned_until_a_resolver_populates_them() {
    let paths = RuntimePaths::default();
    assert!(paths.application_root.as_os_str().is_empty());
    assert!(paths.plugins.as_os_str().is_empty());
    assert!(paths.translations.as_os_str().is_empty());
    assert!(paths.certificates.as_os_str().is_empty());
}
