use super::*;
use serde::Serialize;

#[derive(Serialize)]
struct User {
    id: u32,
    name: String,
}

#[test]
fn success_serializes_structs_and_metadata_in_stable_envelope() {
    let response = ApiResponse::success_with(
        &[User {
            id: 7,
            name: "Ada \"Lovelace\"".into(),
        }],
        200,
        "Users",
        "Loaded",
    )
    .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.content_type, "application/json; charset=utf-8");
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        r#"{"data":[{"id":7,"name":"Ada \"Lovelace\""}],"meta":{"message":"Loaded","title":"Users"}}"#
    );
}

#[test]
fn error_serialization_and_status_rules_match_public_api_contract() {
    let response = ApiResponse::error(
        422,
        &[ApiError {
            title: "Validation Failed".into(),
            detail: "Email is required".into(),
            field: Some("email".into()),
        }],
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        r#"{"errors":[{"detail":"Email is required","source":{"field":"email"},"status":"422","title":"Validation Failed"}]}"#
    );
    assert!(matches!(
        ApiResponse::success_with(&(), 404, "", ""),
        Err(HttpError::InvalidStatus { .. })
    ));
    assert!(matches!(
        ApiResponse::error(422, &[]),
        Err(HttpError::EmptyErrors)
    ));
}

#[test]
fn no_content_response_has_no_body() {
    let response = ApiResponse::success_with(&"ignored", 204, "", "").unwrap();
    assert_eq!(response.status, 204);
    assert!(response.body.is_empty());
}
