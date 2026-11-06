use super::*;

const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn cookie_transport_round_trips_identifier_and_attributes() {
    let transport = SessionIdentifierTransport::cookie(CookieTransportOptions {
        name: "kotavel_session".into(),
        path: "/".into(),
        domain: Some("example.test".into()),
        max_age_seconds: Some(600),
        http_only: true,
        secure: true,
        same_site: SameSite::None,
    })
    .unwrap();
    assert_eq!(
        transport
            .read(Some(&format!("other=x; kotavel_session={ID}")), None)
            .as_deref(),
        Some(ID)
    );
    assert_eq!(
        transport.response_header(ID).unwrap(),
        (
            "Set-Cookie".into(),
            format!(
                "kotavel_session={ID}; Path=/; Domain=example.test; Max-Age=600; HttpOnly; Secure; SameSite=None"
            )
        )
    );
}

#[test]
fn header_transport_rejects_invalid_ids_and_cookie_policy() {
    let transport = SessionIdentifierTransport::header("X-Kotavel-Session").unwrap();
    assert_eq!(transport.read(None, Some(ID)).as_deref(), Some(ID));
    assert_eq!(transport.read(None, Some("bad")), None);
    assert!(
        SessionIdentifierTransport::cookie(CookieTransportOptions {
            secure: false,
            same_site: SameSite::None,
            ..Default::default()
        })
        .is_err()
    );
}
