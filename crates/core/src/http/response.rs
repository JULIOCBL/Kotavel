use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type JsonValue = Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub headers: BTreeMap<String, String>,
}

impl Response {
    pub fn new(status: u16, content_type: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            content_type: content_type.into(),
            body: body.into(),
            headers: BTreeMap::new(),
        }
    }
}

impl Default for Response {
    fn default() -> Self {
        Self::new(200, "text/plain; charset=utf-8", Vec::new())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub title: String,
    pub detail: String,
    pub field: Option<String>,
}

#[derive(Debug)]
pub enum HttpError {
    InvalidStatus { status: u16, expected: &'static str },
    EmptyErrors,
    Json(serde_json::Error),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStatus { status, expected } => {
                write!(formatter, "HTTP status {status} must be {expected}")
            }
            Self::EmptyErrors => {
                formatter.write_str("API error response requires at least one error")
            }
            Self::Json(error) => write!(formatter, "Could not serialize JSON response: {error}"),
        }
    }
}

impl std::error::Error for HttpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

pub struct ApiResponse;

impl ApiResponse {
    pub fn success<T: Serialize>(data: &T) -> Result<Response, HttpError> {
        Self::success_with(data, 200, "", "")
    }

    pub fn success_with<T: Serialize>(
        data: &T,
        status: u16,
        title: &str,
        message: &str,
    ) -> Result<Response, HttpError> {
        if !(200..=299).contains(&status) {
            return Err(HttpError::InvalidStatus {
                status,
                expected: "between 200 and 299",
            });
        }
        if status == 204 {
            return Ok(Response::new(
                status,
                "application/json; charset=utf-8",
                Vec::new(),
            ));
        }
        let mut envelope = Map::new();
        envelope.insert(
            "data".into(),
            serde_json::to_value(data).map_err(HttpError::Json)?,
        );
        if !title.is_empty() || !message.is_empty() {
            let mut meta = Map::new();
            if !message.is_empty() {
                meta.insert("message".into(), Value::String(message.into()));
            }
            if !title.is_empty() {
                meta.insert("title".into(), Value::String(title.into()));
            }
            envelope.insert("meta".into(), Value::Object(meta));
        }
        serialize_response(status, Value::Object(envelope))
    }

    pub fn error(status: u16, errors: &[ApiError]) -> Result<Response, HttpError> {
        if !(400..=599).contains(&status) {
            return Err(HttpError::InvalidStatus {
                status,
                expected: "between 400 and 599",
            });
        }
        if errors.is_empty() {
            return Err(HttpError::EmptyErrors);
        }
        let status_string = status.to_string();
        let values = errors
            .iter()
            .map(|error| {
                let mut item = Map::new();
                item.insert("detail".into(), Value::String(error.detail.clone()));
                if let Some(field) = &error.field {
                    let mut source = Map::new();
                    source.insert("field".into(), Value::String(field.clone()));
                    item.insert("source".into(), Value::Object(source));
                }
                item.insert("status".into(), Value::String(status_string.clone()));
                item.insert("title".into(), Value::String(error.title.clone()));
                Value::Object(item)
            })
            .collect();
        let mut envelope = Map::new();
        envelope.insert("errors".into(), Value::Array(values));
        serialize_response(status, Value::Object(envelope))
    }

    pub fn single_error(status: u16, title: &str, message: &str) -> Result<Response, HttpError> {
        Self::error(
            status,
            &[ApiError {
                title: title.into(),
                detail: message.into(),
                field: None,
            }],
        )
    }
}

fn serialize_response(status: u16, value: Value) -> Result<Response, HttpError> {
    let body = serde_json::to_vec(&value).map_err(HttpError::Json)?;
    Ok(Response::new(
        status,
        "application/json; charset=utf-8",
        body,
    ))
}

#[cfg(test)]
mod tests {
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
}
