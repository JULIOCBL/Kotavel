use super::*;

#[test]
fn parses_json_query_headers_and_route_parameters() {
    let request = Request::new(RequestData {
        method: Method::Post,
        kind: RouteKind::Api,
        path: "/api/items/42".into(),
        body: r#"{"name":"  Kotavel  ","profile":{"city":" Mérida "},"tags":[" one ","two"]}"#
            .as_bytes()
            .to_vec(),
        raw_method: "POST".into(),
        query_string: "page=2&search=blue+tools&name=query-name&empty=%20%20".into(),
        headers: [
            (
                "Content-Type".into(),
                "application/json; charset=utf-8".into(),
            ),
            ("X-Trace".into(), "abc".into()),
        ]
        .into(),
        route_parameters: [("id".into(), "42".into())].into(),
    });
    assert_eq!(request.input("name"), Some("Kotavel"));
    assert_eq!(request.input("profile.city"), Some("Mérida"));
    assert_eq!(request.input("tags.0"), Some("one"));
    assert_eq!(request.query("search"), Some("blue tools"));
    assert_eq!(request.input("page"), Some("2"));
    assert_eq!(request.route("id"), Some("42"));
    assert_eq!(request.header("x-TRACE"), Some("abc"));
    assert!(request.has("empty") && !request.filled("empty"));
    assert_eq!(
        request.all().get("name").map(String::as_str),
        Some("Kotavel")
    );
}

#[test]
fn malformed_json_is_marked_and_null_body_values_shadow_query_values() {
    let malformed = Request::new(RequestData {
        body: r#"{"name": "#.as_bytes().to_vec(),
        headers: [("content-type".into(), "application/json".into())].into(),
        ..RequestData::default()
    });
    assert!(!malformed.has_valid_input());

    let null = Request::new(RequestData {
        body: r#"{"name":null}"#.as_bytes().to_vec(),
        query_string: "name=query".into(),
        headers: [("content-type".into(), "application/json".into())].into(),
        ..RequestData::default()
    });
    assert!(null.exists("name") && null.is_null("name"));
    assert_eq!(null.input("name"), None);
}

#[test]
fn parses_urlencoded_and_binary_multipart_fields() {
    let form = Request::new(RequestData {
        body: b"name=Kotavel+Core&blank=%20".to_vec(),
        headers: [(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        )]
        .into(),
        ..RequestData::default()
    });
    assert_eq!(form.input("name"), Some("Kotavel Core"));
    assert!(form.exists("blank") && !form.filled("blank"));

    let mut body = b"--unit\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"sample.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    body.extend_from_slice(&[0, 1, 2, 255]);
    body.extend_from_slice(b"\r\n--unit--\r\n");
    let request = Request::new(RequestData {
        body,
        headers: [(
            "content-type".into(),
            "multipart/form-data; boundary=unit".into(),
        )]
        .into(),
        ..RequestData::default()
    });
    assert_eq!(
        request.file("upload").map(|file| file.name.as_str()),
        Some("sample.bin")
    );
    assert_eq!(
        request.file("upload").map(|file| file.contents.as_slice()),
        Some(&[0, 1, 2, 255][..])
    );
}
