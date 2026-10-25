use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::session::{
    LinkedSessionContext, SessionContextResolver, SessionLinkGrant, SessionScope, StoreError,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouteKind {
    Api,
    #[default]
    Web,
}

#[derive(Clone, Debug)]
pub struct RequestData {
    pub method: Method,
    pub kind: RouteKind,
    pub path: String,
    pub body: Vec<u8>,
    pub raw_method: String,
    pub query_string: String,
    pub headers: BTreeMap<String, String>,
    pub route_parameters: BTreeMap<String, String>,
}

impl Default for RequestData {
    fn default() -> Self {
        Self {
            method: Method::Get,
            kind: RouteKind::Web,
            path: "/".into(),
            body: Vec::new(),
            raw_method: String::new(),
            query_string: String::new(),
            headers: BTreeMap::new(),
            route_parameters: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadedFile {
    pub name: String,
    pub content_type: String,
    pub contents: Vec<u8>,
}

/// Owns request transport data and parsed values for the full handler lifetime.
#[derive(Clone, Debug)]
pub struct Request {
    method: Method,
    kind: RouteKind,
    path: String,
    body: Vec<u8>,
    raw_method: String,
    query_string: String,
    headers: BTreeMap<String, String>,
    route_parameters: BTreeMap<String, String>,
    query_values: BTreeMap<String, Option<String>>,
    body_values: BTreeMap<String, Option<String>>,
    body_arrays: BTreeSet<String>,
    body_objects: BTreeSet<String>,
    body_array_values: BTreeMap<String, Vec<String>>,
    body_object_keys: BTreeMap<String, Vec<String>>,
    files: BTreeMap<String, UploadedFile>,
    input_valid: bool,
    session: RequestSessionState,
    session_context: RequestSessionContextState,
}

#[derive(Clone, Default)]
struct RequestSessionState(Arc<Mutex<Option<SessionScope>>>);

impl std::fmt::Debug for RequestSessionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RequestSessionState")
    }
}

#[derive(Clone, Default)]
struct RequestSessionContextState(Arc<Mutex<Option<Arc<SessionContextResolver>>>>);

impl std::fmt::Debug for RequestSessionContextState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RequestSessionContextState")
    }
}

#[derive(Debug)]
pub enum SessionAccessError {
    Unavailable,
    Store(StoreError),
}

impl std::fmt::Display for SessionAccessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => {
                formatter.write_str("session is unavailable outside session middleware")
            }
            Self::Store(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SessionAccessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable => None,
            Self::Store(error) => Some(error),
        }
    }
}

/// Laravel-style operations scoped to the session installed on one request.
/// Each call holds the request's session lock only for that operation.
pub struct Session<'a> {
    request: &'a Request,
}

impl Session<'_> {
    fn access<T>(
        &self,
        operation: impl FnOnce(&mut SessionScope) -> T,
    ) -> Result<T, SessionAccessError> {
        self.request.with_session(operation)
    }

    pub fn id(&self) -> Result<String, SessionAccessError> {
        self.access(|session| session.id().to_owned())
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, SessionAccessError> {
        self.access(|session| session.get(key).cloned())?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| SessionAccessError::Store(StoreError::Json(error)))
    }

    pub fn get_value(&self, key: &str) -> Result<Option<Value>, SessionAccessError> {
        self.get(key)
    }

    pub fn has(&self, key: &str) -> Result<bool, SessionAccessError> {
        self.access(|session| session.has(key))
    }

    pub fn exists(&self, key: &str) -> Result<bool, SessionAccessError> {
        self.access(|session| session.exists(key))
    }

    pub fn all(&self) -> Result<serde_json::Map<String, Value>, SessionAccessError> {
        self.access(|session| session.all())
    }

    pub fn put<T: Serialize>(
        &self,
        key: impl Into<String>,
        value: T,
    ) -> Result<(), SessionAccessError> {
        let value = serde_json::to_value(value)
            .map_err(StoreError::Json)
            .map_err(SessionAccessError::Store)?;
        self.access(|session| session.put(key, value))?
            .map_err(SessionAccessError::Store)
    }

    pub fn forget(&self, key: &str) -> Result<(), SessionAccessError> {
        self.access(|session| session.forget(key))?
            .map_err(SessionAccessError::Store)
    }

    pub fn increment(
        &self,
        key: impl Into<String>,
        amount: u64,
    ) -> Result<u64, SessionAccessError> {
        self.access(|session| session.increment(key, amount))?
            .map_err(SessionAccessError::Store)
    }

    pub fn flush(&self) -> Result<(), SessionAccessError> {
        self.access(SessionScope::flush)
    }

    pub fn csrf_token(&self) -> Result<String, SessionAccessError> {
        self.access(SessionScope::csrf_token)?
            .map_err(SessionAccessError::Store)
    }

    pub fn verify_csrf_token(&self, candidate: &str) -> Result<bool, SessionAccessError> {
        self.access(|session| session.verify_csrf_token(candidate))
    }

    pub fn regenerate(&self) -> Result<(), SessionAccessError> {
        self.access(SessionScope::regenerate)?
            .map_err(SessionAccessError::Store)
    }

    pub fn invalidate(&self) -> Result<(), SessionAccessError> {
        self.access(SessionScope::invalidate)?
            .map_err(SessionAccessError::Store)
    }
}

impl Request {
    pub fn new(data: RequestData) -> Self {
        let mut request = Self {
            method: data.method,
            kind: data.kind,
            path: data.path,
            body: data.body,
            raw_method: data.raw_method,
            query_string: data.query_string,
            headers: data
                .headers
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value))
                .collect(),
            route_parameters: data.route_parameters,
            query_values: BTreeMap::new(),
            body_values: BTreeMap::new(),
            body_arrays: BTreeSet::new(),
            body_objects: BTreeSet::new(),
            body_array_values: BTreeMap::new(),
            body_object_keys: BTreeMap::new(),
            files: BTreeMap::new(),
            input_valid: true,
            session: RequestSessionState::default(),
            session_context: RequestSessionContextState::default(),
        };

        parse_url_encoded(&request.query_string, &mut request.query_values);
        let content_type = request
            .header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if content_type.starts_with("application/json") {
            if !request.body.is_empty() {
                match serde_json::from_slice::<Value>(&request.body) {
                    Ok(value) => flatten_json(
                        &value,
                        "",
                        &mut request.body_values,
                        &mut request.body_arrays,
                        &mut request.body_objects,
                        &mut request.body_array_values,
                        &mut request.body_object_keys,
                    ),
                    Err(_) => request.input_valid = false,
                }
            }
        } else if content_type.starts_with("application/x-www-form-urlencoded") {
            parse_url_encoded(
                &String::from_utf8_lossy(&request.body),
                &mut request.body_values,
            );
        } else if content_type.starts_with("multipart/form-data") {
            if let Some(boundary) = multipart_boundary(&content_type) {
                parse_multipart(
                    &request.body,
                    boundary.as_bytes(),
                    &mut request.body_values,
                    &mut request.files,
                );
            }
        }
        request
    }

    pub fn method(&self) -> Method {
        self.method
    }
    pub fn kind(&self) -> RouteKind {
        self.kind
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn raw_method(&self) -> &str {
        &self.raw_method
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn query_string(&self) -> &str {
        &self.query_string
    }
    pub fn has_valid_input(&self) -> bool {
        self.input_valid
    }

    /// Body values shadow query values, including explicit JSON nulls.
    pub fn input(&self, key: &str) -> Option<&str> {
        if let Some(value) = self.body_values.get(key) {
            return value.as_deref();
        }
        if let Some(file) = self.files.get(key) {
            return Some(&file.name);
        }
        self.query(key)
    }

    pub fn query(&self, key: &str) -> Option<&str> {
        self.query_values.get(key).and_then(Option::as_deref)
    }

    pub fn route(&self, key: &str) -> Option<&str> {
        self.route_parameters.get(key).map(String::as_str)
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Access the request-scoped session while session middleware is running.
    pub fn with_session<T>(
        &self,
        operation: impl FnOnce(&mut SessionScope) -> T,
    ) -> Result<T, SessionAccessError> {
        self.lock_session()
            .as_mut()
            .map(operation)
            .ok_or(SessionAccessError::Unavailable)
    }

    /// Returns a request-scoped session facade while session middleware is
    /// active. The facade never exposes references beyond its short lock.
    pub fn session(&self) -> Result<Session<'_>, SessionAccessError> {
        if self.lock_session().is_some() {
            Ok(Session { request: self })
        } else {
            Err(SessionAccessError::Unavailable)
        }
    }

    /// Returns shared context access only when the active session has redeemed
    /// a valid link grant. Routes without context-aware session middleware see
    /// `None` and remain isolated by default.
    pub fn session_context(&self) -> Result<Option<LinkedSessionContext>, SessionAccessError> {
        let session_id = self.with_session(|session| session.id().to_owned())?;
        let resolver = self.lock_session_context().clone();
        let Some(resolver) = resolver else {
            return Ok(None);
        };
        resolver
            .linked_session(&session_id)
            .map_err(SessionAccessError::Store)
    }

    /// Redeems a short-lived grant for this request's session. Protect the route
    /// that accepts grants; possession of the grant authorizes linking one session.
    pub fn redeem_session_context_link(
        &self,
        grant: &SessionLinkGrant,
    ) -> Result<bool, SessionAccessError> {
        let committed = self
            .with_session(|session| session.commit())?
            .map_err(SessionAccessError::Store)?;
        if !committed {
            return Ok(false);
        }
        let session_id = self.with_session(|session| session.id().to_owned())?;
        let resolver = self
            .lock_session_context()
            .clone()
            .ok_or(SessionAccessError::Unavailable)?;
        resolver
            .redeem_link_grant(grant, &session_id)
            .map_err(SessionAccessError::Store)
    }

    pub(crate) fn install_session(&self, session: SessionScope) -> bool {
        let mut slot = self.lock_session();
        if slot.is_some() {
            false
        } else {
            *slot = Some(session);
            true
        }
    }

    pub(crate) fn take_session(&self) -> Option<SessionScope> {
        self.lock_session().take()
    }

    pub(crate) fn install_session_context_resolver(
        &self,
        resolver: Arc<SessionContextResolver>,
    ) -> bool {
        let mut slot = self.lock_session_context();
        if slot.is_some() {
            false
        } else {
            *slot = Some(resolver);
            true
        }
    }

    pub(crate) fn take_session_context_resolver(&self) {
        self.lock_session_context().take();
    }

    fn lock_session(&self) -> MutexGuard<'_, Option<SessionScope>> {
        self.session
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_session_context(&self) -> MutexGuard<'_, Option<Arc<SessionContextResolver>>> {
        self.session_context
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn file(&self, key: &str) -> Option<&UploadedFile> {
        self.files.get(key)
    }

    pub fn exists(&self, key: &str) -> bool {
        self.body_values.contains_key(key)
            || self.query_values.contains_key(key)
            || self.files.contains_key(key)
            || self.body_arrays.contains(key)
            || self.body_objects.contains(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.exists(key)
    }

    pub fn is_null(&self, key: &str) -> bool {
        self.body_values.get(key).is_some_and(Option::is_none)
            || (!self.body_values.contains_key(key)
                && self.query_values.get(key).is_some_and(Option::is_none))
    }

    pub fn is_array(&self, key: &str) -> bool {
        self.body_arrays.contains(key)
    }
    pub fn is_object(&self, key: &str) -> bool {
        self.body_objects.contains(key)
    }

    pub fn filled(&self, key: &str) -> bool {
        if self.is_array(key) {
            return self
                .body_array_values
                .get(key)
                .is_some_and(|values| !values.is_empty());
        }
        if self.is_object(key) {
            return self
                .body_object_keys
                .get(key)
                .is_some_and(|keys| !keys.is_empty());
        }
        if let Some(file) = self.file(key) {
            return !file.contents.is_empty();
        }
        self.input(key).is_some_and(|value| !value.is_empty())
    }

    pub fn array_values(&self, key: &str) -> &[String] {
        self.body_array_values.get(key).map_or(&[], Vec::as_slice)
    }

    pub fn array_keys(&self, key: &str) -> &[String] {
        self.body_object_keys.get(key).map_or(&[], Vec::as_slice)
    }

    pub fn all(&self) -> BTreeMap<String, String> {
        let mut values = BTreeMap::new();
        for (key, value) in &self.query_values {
            if let Some(value) = value {
                values.insert(key.clone(), value.clone());
            }
        }
        for (key, value) in &self.body_values {
            if let Some(value) = value {
                values.insert(key.clone(), value.clone());
            }
        }
        for (key, file) in &self.files {
            values.insert(key.clone(), file.name.clone());
        }
        values
    }
}

fn trim_ascii(value: &str) -> &str {
    value.trim_matches(|character: char| character.is_ascii_whitespace())
}

fn store_value(values: &mut BTreeMap<String, Option<String>>, key: String, value: Option<String>) {
    let value = value
        .map(|value| trim_ascii(&value).to_owned())
        .filter(|value| !value.is_empty());
    values.insert(key, value);
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    decoded.push((high << 4) | low);
                    index += 2;
                } else {
                    decoded.push(bytes[index]);
                }
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn parse_url_encoded(encoded: &str, values: &mut BTreeMap<String, Option<String>>) {
    for pair in encoded.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_component(key);
        if !key.is_empty() {
            store_value(values, key, Some(decode_component(value)));
        }
    }
}

fn scalar_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

fn flatten_json(
    value: &Value,
    key: &str,
    values: &mut BTreeMap<String, Option<String>>,
    arrays: &mut BTreeSet<String>,
    objects: &mut BTreeSet<String>,
    array_values: &mut BTreeMap<String, Vec<String>>,
    object_keys: &mut BTreeMap<String, Vec<String>>,
) {
    match value {
        Value::Object(object) => {
            objects.insert(key.to_owned());
            for (name, child) in object {
                object_keys
                    .entry(key.to_owned())
                    .or_default()
                    .push(name.clone());
                let child_key = if key.is_empty() {
                    name.clone()
                } else {
                    format!("{key}.{name}")
                };
                flatten_json(
                    child,
                    &child_key,
                    values,
                    arrays,
                    objects,
                    array_values,
                    object_keys,
                );
            }
        }
        Value::Array(array) => {
            arrays.insert(key.to_owned());
            for (index, child) in array.iter().enumerate() {
                let child_key = if key.is_empty() {
                    index.to_string()
                } else {
                    format!("{key}.{index}")
                };
                array_values
                    .entry(key.to_owned())
                    .or_default()
                    .push(match child {
                        Value::Object(_) | Value::Array(_) => "[complex]".into(),
                        Value::Null => String::new(),
                        _ => scalar_value(child),
                    });
                flatten_json(
                    child,
                    &child_key,
                    values,
                    arrays,
                    objects,
                    array_values,
                    object_keys,
                );
            }
        }
        Value::Null => store_value(values, key.to_owned(), None),
        _ => store_value(values, key.to_owned(), Some(scalar_value(value))),
    }
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    let (_, parameters) = content_type.split_once(';')?;
    parameters
        .split(';')
        .find_map(|parameter| {
            let (name, value) = parameter.trim().split_once('=')?;
            if !name.eq_ignore_ascii_case("boundary") {
                return None;
            }
            Some(value.trim().trim_matches('"').to_owned())
        })
        .filter(|boundary| !boundary.is_empty())
}

fn quoted_parameter(text: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = text.find(&needle)? + needle.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_owned())
}

fn parse_multipart(
    body: &[u8],
    boundary: &[u8],
    fields: &mut BTreeMap<String, Option<String>>,
    files: &mut BTreeMap<String, UploadedFile>,
) {
    let delimiter = [b"--".as_slice(), boundary].concat();
    let mut cursor = 0;
    while let Some(relative_start) = find_bytes(&body[cursor..], &delimiter) {
        cursor += relative_start + delimiter.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        if body.get(cursor..cursor + 2) == Some(b"\r\n") {
            cursor += 2;
        }
        let Some(relative_headers_end) = find_bytes(&body[cursor..], b"\r\n\r\n") else {
            break;
        };
        let headers_end = cursor + relative_headers_end;
        let headers = String::from_utf8_lossy(&body[cursor..headers_end]);
        let content_start = headers_end + 4;
        let next_delimiter = [b"\r\n".as_slice(), delimiter.as_slice()].concat();
        let Some(relative_content_end) = find_bytes(&body[content_start..], &next_delimiter) else {
            break;
        };
        let content_end = content_start + relative_content_end;
        let mut disposition = String::new();
        let mut content_type = String::new();
        for line in headers.lines() {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-disposition") {
                    disposition = value.trim().to_owned();
                }
                if name.eq_ignore_ascii_case("content-type") {
                    content_type = value.trim().to_ascii_lowercase();
                }
            }
        }
        let name = quoted_parameter(&disposition, "name").unwrap_or_default();
        let filename = quoted_parameter(&disposition, "filename").unwrap_or_default();
        if !name.is_empty() && !filename.is_empty() {
            files.insert(
                name,
                UploadedFile {
                    name: filename,
                    content_type,
                    contents: body[content_start..content_end].to_vec(),
                },
            );
        } else if !name.is_empty() {
            store_value(
                fields,
                name,
                Some(String::from_utf8_lossy(&body[content_start..content_end]).into_owned()),
            );
        }
        cursor = content_end + 2;
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
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
}
