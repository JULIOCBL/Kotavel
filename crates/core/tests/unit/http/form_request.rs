use super::*;
use std::collections::BTreeMap;
struct Rules(ValidationRules);
impl FormRequest for Rules {
    fn rules(&self) -> ValidationRules {
        self.0.clone()
    }
}
fn request(body: &str) -> Request {
    Request::new(super::super::RequestData {
        kind: RouteKind::Api,
        body: body.as_bytes().to_vec(),
        headers: [("content-type".into(), "application/json".into())].into(),
        ..Default::default()
    })
}
#[test]
fn validates_scalar_nested_array_conditional_and_wildcard_rules() {
    let request = request(
        r#"{"name":"Ada","email":"ada@example.com","amount":"12.50","roles":["admin","editor"],"rows":[{"code":"row_1"}],"mode":"card","card":"123"}"#,
    );
    let form = Rules(BTreeMap::from([
        ("name".into(), vec!["required".into(), "alpha".into()]),
        ("email".into(), vec!["email".into()]),
        (
            "amount".into(),
            vec!["numeric".into(), "decimal:2".into(), "between:10,20".into()],
        ),
        (
            "roles".into(),
            vec![
                "array".into(),
                "contains:admin,editor".into(),
                "distinct".into(),
            ],
        ),
        ("rows.*.code".into(), vec!["alpha_dash".into()]),
        ("card".into(), vec!["required_if:mode,card".into()]),
    ]));
    let result = validate_form(&request, &form).unwrap();
    assert!(result.is_valid(), "{:?}", result.errors());
    assert_eq!(
        result
            .validated()
            .unwrap()
            .get("rows.0.code")
            .map(String::as_str),
        Some("row_1")
    );
}

#[test]
fn validates_dates_urls_numeric_ranges_and_object_rules() {
    let request = request(
        r#"{"site":"https://example.com/path","local":"http://localhost","ip":"127.0.0.1","clock":"17:30:05","date":"2026-01-01","later":"2026-02-01","zone":"UTC","amount":"12.50","digits":"007","token":"550e8400-e29b-41d4-a716-446655440000","ulid":"01ARZ3NDEKTSV4RRFFQ69G5FAV","settings":{"theme":"dark","locale":"es"},"choices":["red","blue"],"choice":"blue"}"#,
    );
    let form = Rules(BTreeMap::from([
        ("site".into(), vec!["url".into()]),
        ("local".into(), vec!["active_url".into()]),
        ("ip".into(), vec!["ip".into(), "ipv4".into()]),
        ("clock".into(), vec!["date_format:H:i:s".into()]),
        (
            "date".into(),
            vec!["date".into(), "date_equals:2026-01-01".into()],
        ),
        (
            "later".into(),
            vec![
                "date".into(),
                "after:date".into(),
                "before:2027-01-01".into(),
            ],
        ),
        ("zone".into(), vec!["timezone".into()]),
        (
            "amount".into(),
            vec![
                "numeric".into(),
                "decimal:2".into(),
                "between:10,20".into(),
                "multiple_of:0.25".into(),
            ],
        ),
        (
            "digits".into(),
            vec![
                "integer".into(),
                "digits:3".into(),
                "digits_between:2,4".into(),
            ],
        ),
        ("token".into(), vec!["uuid".into()]),
        ("ulid".into(), vec!["ulid".into()]),
        (
            "settings".into(),
            vec![
                "array".into(),
                "array_keys:theme,locale".into(),
                "required_array_keys:theme,locale".into(),
                "in_array_keys:locale".into(),
                "size:2".into(),
            ],
        ),
        (
            "choices".into(),
            vec![
                "array".into(),
                "contains:red,blue".into(),
                "doesnt_contain:green".into(),
                "distinct".into(),
            ],
        ),
        ("choice".into(), vec!["in_array:choices.*".into()]),
    ]));
    let result = validate_form(&request, &form).unwrap();
    assert!(result.is_valid(), "{:?}", result.errors());
}

#[test]
fn validates_uploaded_image_mime_extension_dimensions_and_size() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 1, 0, 0, 0, 1]);
    let mut body = b"--unit\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"pixel.png\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
    body.extend_from_slice(&png);
    body.extend_from_slice(b"\r\n--unit--\r\n");
    let request = Request::new(super::super::RequestData {
        kind: RouteKind::Api,
        body,
        headers: [(
            "content-type".into(),
            "multipart/form-data; boundary=unit".into(),
        )]
        .into(),
        ..Default::default()
    });
    let form = Rules(BTreeMap::from([(
        "upload".into(),
        vec![
            "required".into(),
            "file".into(),
            "image".into(),
            "mimes:png".into(),
            "mimetypes:image/png".into(),
            "extensions:png".into(),
            "dimensions:width=1,height=1".into(),
            "max:2".into(),
        ],
    )]));
    let result = validate_form(&request, &form).unwrap();
    assert!(result.is_valid(), "{:?}", result.errors());
    assert_eq!(
        result
            .validated()
            .unwrap()
            .get("upload")
            .map(String::as_str),
        Some("pixel.png")
    );
}

#[test]
fn form_handler_runs_action_only_with_validated_input() {
    let policy = Rules(BTreeMap::from([(
        "name".into(),
        vec!["required".into(), "string".into()],
    )]));
    let handler = form_handler(
        move |_| Rules(policy.0.clone()),
        |_, validation| ApiResponse::success(validation.validated().unwrap()).unwrap(),
    );
    let accepted = handler(&request(r#"{"name":"Kotavel","ignored":"drop"}"#));
    assert_eq!(
        String::from_utf8(accepted.body).unwrap(),
        r#"{"data":{"name":"Kotavel"}}"#
    );
    let rejected = handler(&request(r#"{"name":" "}"#));
    assert_eq!(rejected.status, 422);
}

#[test]
fn authorization_validation_errors_and_unknown_rules_are_explicit() {
    struct Denied;
    impl FormRequest for Denied {
        fn authorize(&self, _: &Request) -> bool {
            false
        }
        fn rules(&self) -> ValidationRules {
            ValidationRules::new()
        }
    }
    let result = validate_form(&request("{}"), &Denied).unwrap();
    assert_eq!(result.error_response().unwrap().status, 403);
    let bad = Rules(BTreeMap::from([("x".into(), vec!["not_a_rule".into()])]));
    assert!(matches!(
        validate_form(&request(r#"{"x":"a"}"#), &bad),
        Err(FormRequestError::UnsupportedRule(_))
    ));
}
#[test]
fn invalid_json_and_field_errors_use_standard_api_envelopes() {
    let malformed = Request::new(super::super::RequestData {
        kind: RouteKind::Api,
        body: b"{".to_vec(),
        headers: [("content-type".into(), "application/json".into())].into(),
        ..Default::default()
    });
    let form = Rules(ValidationRules::from([(
        "name".into(),
        vec!["required".into()],
    )]));
    let invalid = validate_form(&malformed, &form).unwrap();
    assert_eq!(invalid.error_response().unwrap().status, 400);
    let request = request(r#"{"name":" "}"#);
    let error = validate_form(&request, &form).unwrap();
    assert_eq!(error.error_response().unwrap().status, 422);
    assert!(error.errors()[0].detail.contains("required"));
}
