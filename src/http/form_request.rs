use super::{ApiError, ApiResponse, Handler, HttpError, Request, Response, RouteKind};
use chrono_tz::Tz;
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
use regex::Regex;
use std::{collections::BTreeMap, net::ToSocketAddrs, str::FromStr};
use url::Url;

pub type ValidationRules = BTreeMap<String, Vec<String>>;

/// Application-specific authorization and validation policy for a request.
pub trait FormRequest {
    fn authorize(&self, _request: &Request) -> bool {
        true
    }
    fn rules(&self) -> ValidationRules;
    fn current_password_matches(&self, _password: &str, _guard: &str) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub title: String,
    pub detail: String,
    pub field: Option<String>,
}

#[derive(Debug)]
pub enum FormRequestError {
    NotValidated,
    ValidationFailed,
    UnsupportedRule(String),
    Response(HttpError),
}

impl std::fmt::Display for FormRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotValidated => {
                f.write_str("Validate the form request before reading its result")
            }
            Self::ValidationFailed => {
                f.write_str("Validated input is unavailable when the form request has errors")
            }
            Self::UnsupportedRule(rule) => {
                write!(f, "Unsupported FormRequest validation rule: {rule}")
            }
            Self::Response(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for FormRequestError {}

pub struct FormValidation {
    validated: BTreeMap<String, String>,
    errors: Vec<ValidationError>,
    authorized: bool,
    validation_ran: bool,
    input_valid: bool,
    kind: RouteKind,
}

impl FormValidation {
    pub fn is_valid(&self) -> bool {
        self.validation_ran && self.authorized && self.errors.is_empty() && self.input_valid
    }
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }
    pub fn validated(&self) -> Result<&BTreeMap<String, String>, FormRequestError> {
        if !self.validation_ran {
            return Err(FormRequestError::NotValidated);
        }
        if !self.authorized || !self.errors.is_empty() || !self.input_valid {
            return Err(FormRequestError::ValidationFailed);
        }
        Ok(&self.validated)
    }

    pub fn error_response(&self) -> Result<Response, FormRequestError> {
        if !self.validation_ran {
            return Err(FormRequestError::NotValidated);
        }
        if self.authorized && self.errors.is_empty() && self.input_valid {
            return Err(FormRequestError::ValidationFailed);
        }
        let status = if !self.authorized {
            403
        } else if !self.input_valid {
            400
        } else {
            422
        };
        if self.kind == RouteKind::Api {
            if !self.authorized {
                return ApiResponse::single_error(
                    403,
                    "Forbidden",
                    "This action is not authorized.",
                )
                .map_err(FormRequestError::Response);
            }
            if !self.input_valid {
                return ApiResponse::single_error(
                    400,
                    "Bad Request",
                    "The request body contains invalid JSON.",
                )
                .map_err(FormRequestError::Response);
            }
            let errors: Vec<_> = self
                .errors
                .iter()
                .map(|error| ApiError {
                    title: error.title.clone(),
                    detail: error.detail.clone(),
                    field: error.field.clone(),
                })
                .collect();
            return ApiResponse::error(422, &errors).map_err(FormRequestError::Response);
        }
        let title = match status {
            403 => "403 Forbidden",
            400 => "400 Bad Request",
            _ => "422 Unprocessable Entity",
        };
        let mut page = format!(
            "<!doctype html><html><head><title>{title}</title></head><body><h1>{title}</h1><ul>"
        );
        if !self.authorized {
            page.push_str("<li>This action is not authorized.</li>");
        }
        if !self.input_valid {
            page.push_str("<li>The request body contains invalid JSON.</li>");
        }
        for error in &self.errors {
            page.push_str("<li>");
            page.push_str(&escape_html(&error.detail));
            page.push_str("</li>");
        }
        page.push_str("</ul></body></html>");
        Ok(Response::new(status, "text/html; charset=utf-8", page))
    }
}

pub fn validate_form<F: FormRequest>(
    request: &Request,
    form: &F,
) -> Result<FormValidation, FormRequestError> {
    let authorized = form.authorize(request);
    let mut result = FormValidation {
        validated: BTreeMap::new(),
        errors: Vec::new(),
        authorized,
        validation_ran: true,
        input_valid: request.has_valid_input(),
        kind: request.kind(),
    };
    if !authorized {
        return Ok(result);
    }
    if !result.input_valid {
        result.errors.push(ValidationError {
            title: "Validation Failed".into(),
            detail: "The request body contains invalid JSON.".into(),
            field: None,
        });
        return Ok(result);
    }

    let declared = form.rules();
    let values = request.all();
    let mut rules = BTreeMap::new();
    for (pattern, field_rules) in declared {
        if !pattern.contains('*') {
            rules.insert(pattern, field_rules);
        } else {
            for key in values.keys() {
                if path_matches(&pattern, key) {
                    rules.insert(key.clone(), field_rules.clone());
                }
            }
        }
    }

    for (field, field_rules) in rules {
        for encoded in &field_rules {
            let (rule, _) = rule_parts(encoded);
            if !supported_rule(rule) {
                return Err(FormRequestError::UnsupportedRule(rule.into()));
            }
        }
        if field_rules.iter().any(|encoded| {
            let (rule, argument) = rule_parts(encoded);
            excludes_field(request, &field, rule, argument)
        }) {
            continue;
        }

        let present = request.exists(&field);
        let composite = request.is_array(&field) || request.is_object(&field);
        let value = request.input(&field).unwrap_or("");
        let nullable = field_rules.iter().any(|rule| rule == "nullable");
        let sometimes = field_rules.iter().any(|rule| rule == "sometimes");
        if !present && sometimes {
            continue;
        }
        let numeric_attribute = field_rules
            .iter()
            .any(|encoded| matches!(rule_parts(encoded).0, "numeric" | "integer" | "decimal"));
        let bail = field_rules.iter().any(|rule| rule == "bail");
        let null_value = request.is_null(&field);
        let mut field_valid = true;

        for encoded in &field_rules {
            let (rule, argument) = rule_parts(encoded);
            if matches!(
                rule,
                "nullable"
                    | "sometimes"
                    | "exclude"
                    | "exclude_if"
                    | "exclude_unless"
                    | "exclude_with"
                    | "exclude_without"
                    | "bail"
            ) {
                continue;
            }
            if is_conditional_rule(rule) {
                if !conditional_rule_passes(request, &field, rule, argument) {
                    result.errors.push(validation_error(&field, rule, argument));
                    field_valid = false;
                    if bail {
                        break;
                    }
                }
                continue;
            }
            if !present {
                continue;
            }
            if rule == "current_password" {
                if !form.current_password_matches(value, argument) {
                    result.errors.push(validation_error(&field, rule, argument));
                    field_valid = false;
                    if bail {
                        break;
                    }
                }
                continue;
            }
            if null_value {
                if nullable {
                    continue;
                }
                result.errors.push(validation_error(&field, rule, argument));
                field_valid = false;
                if bail {
                    break;
                }
                continue;
            }
            if !accepts(request, &field, value, rule, argument, numeric_attribute) {
                result.errors.push(validation_error(&field, rule, argument));
                field_valid = false;
                if bail {
                    break;
                }
            }
        }

        if field_valid && present {
            if let Some(value) = request.input(&field).filter(|_| !composite) {
                result.validated.insert(field, value.to_owned());
            } else if let Some(file) = request.file(&field) {
                result.validated.insert(field, file.name.clone());
            } else if composite {
                let prefix = format!("{field}.");
                for (key, value) in values.range(prefix.clone()..) {
                    if !key.starts_with(&prefix) {
                        break;
                    }
                    result.validated.insert(key.clone(), value.clone());
                }
            }
        }
    }
    Ok(result)
}

/// Adapts a typed form policy and action into a route handler. Authorization
/// and validation run before the action; failures become standard 403/400/422
/// responses and the action receives only the validated values.
pub fn form_handler<F, Build, Action>(build: Build, action: Action) -> Handler
where
    F: FormRequest + 'static,
    Build: Fn(&Request) -> F + Send + Sync + 'static,
    Action: Fn(&F, &FormValidation) -> Response + Send + Sync + 'static,
{
    std::sync::Arc::new(move |request| {
        let form = build(request);
        let validation = validate_form(request, &form).unwrap_or_else(|error| panic!("{error}"));
        if validation.is_valid() {
            action(&form, &validation)
        } else {
            validation.error_response().unwrap_or_default()
        }
    })
}

fn rule_parts(encoded: &str) -> (&str, &str) {
    encoded.split_once(':').unwrap_or((encoded, ""))
}
fn split(value: &str, separator: char) -> Vec<&str> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(separator).collect()
    }
}
fn path_matches(pattern: &str, key: &str) -> bool {
    let pattern: Vec<_> = pattern.split('.').collect();
    let key: Vec<_> = key.split('.').collect();
    pattern.len() == key.len()
        && pattern
            .iter()
            .zip(key)
            .all(|(pattern, key)| *pattern == "*" || *pattern == key)
}
fn in_values(value: &str, argument: &str) -> bool {
    split(argument, ',').contains(&value)
}
fn chars(value: &str) -> usize {
    value.chars().count()
}
fn parse_number(value: &str) -> Option<f64> {
    let number = value.parse::<f64>().ok()?;
    number.is_finite().then_some(number)
}
fn parse_integer(value: &str) -> bool {
    !value.is_empty() && value.parse::<i64>().is_ok()
}
fn validate_regex(value: &str, pattern: &str) -> bool {
    let (pattern, insensitive) = if pattern.starts_with('/') {
        if let Some(end) = pattern.rfind('/').filter(|end| *end > 0) {
            (&pattern[1..end], pattern[end + 1..].contains('i'))
        } else {
            (pattern, false)
        }
    } else {
        (pattern, false)
    };
    let pattern = if insensitive {
        format!("(?i:{pattern})")
    } else {
        pattern.to_owned()
    };
    Regex::new(&pattern).is_ok_and(|regex| {
        regex.is_match(value)
            && regex
                .find(value)
                .is_some_and(|m| m.start() == 0 && m.end() == value.len())
    })
}
fn supported_rule(rule: &str) -> bool {
    matches!(
        rule,
        "required"
            | "nullable"
            | "string"
            | "array"
            | "list"
            | "integer"
            | "numeric"
            | "boolean"
            | "accepted"
            | "declined"
            | "email"
            | "file"
            | "image"
            | "mimes"
            | "mimetypes"
            | "extensions"
            | "encoding"
            | "dimensions"
            | "url"
            | "active_url"
            | "ip"
            | "ipv4"
            | "ipv6"
            | "json"
            | "ascii"
            | "alpha"
            | "alpha_num"
            | "alpha_dash"
            | "lowercase"
            | "uppercase"
            | "in"
            | "not_in"
            | "enum"
            | "any_of"
            | "same"
            | "different"
            | "confirmed"
            | "starts_with"
            | "ends_with"
            | "doesnt_start_with"
            | "doesnt_end_with"
            | "regex"
            | "not_regex"
            | "hex_color"
            | "mac_address"
            | "uuid"
            | "ulid"
            | "date"
            | "date_format"
            | "date_equals"
            | "after"
            | "after_or_equal"
            | "before"
            | "before_or_equal"
            | "timezone"
            | "min"
            | "max"
            | "size"
            | "between"
            | "decimal"
            | "digits"
            | "digits_between"
            | "min_digits"
            | "max_digits"
            | "gt"
            | "gte"
            | "lt"
            | "lte"
            | "multiple_of"
            | "array_keys"
            | "required_array_keys"
            | "contains"
            | "doesnt_contain"
            | "distinct"
            | "in_array"
            | "in_array_keys"
            | "bail"
            | "sometimes"
            | "exclude"
            | "exclude_if"
            | "exclude_unless"
            | "exclude_with"
            | "exclude_without"
            | "filled"
            | "missing"
            | "missing_if"
            | "missing_unless"
            | "missing_with"
            | "missing_with_all"
            | "present"
            | "present_if"
            | "present_unless"
            | "present_with"
            | "present_with_all"
            | "present_without"
            | "missing_without"
            | "missing_without_all"
            | "prohibited"
            | "prohibited_if"
            | "prohibited_if_accepted"
            | "prohibited_if_declined"
            | "prohibited_unless"
            | "prohibits"
            | "required_if"
            | "required_if_accepted"
            | "required_if_declined"
            | "required_unless"
            | "required_with"
            | "required_with_all"
            | "required_without"
            | "required_without_all"
            | "accepted_if"
            | "declined_if"
            | "current_password"
    )
}
fn is_conditional_rule(rule: &str) -> bool {
    matches!(
        rule,
        "required"
            | "required_if"
            | "required_if_accepted"
            | "required_if_declined"
            | "required_unless"
            | "required_with"
            | "required_with_all"
            | "required_without"
            | "required_without_all"
            | "present"
            | "present_if"
            | "present_unless"
            | "present_with"
            | "present_with_all"
            | "present_without"
            | "missing"
            | "missing_if"
            | "missing_unless"
            | "missing_with"
            | "missing_with_all"
            | "missing_without"
            | "missing_without_all"
            | "filled"
            | "prohibited"
            | "prohibited_if"
            | "prohibited_if_accepted"
            | "prohibited_if_declined"
            | "prohibited_unless"
            | "prohibits"
            | "accepted_if"
            | "declined_if"
    )
}

fn matches_condition(request: &Request, field: &str, rule: &str, argument: &str) -> bool {
    let args = split(argument, ',');
    if args.is_empty() {
        return false;
    }
    let other = request.input(args[0]);
    let values = &args[1..];
    if matches!(rule, "required_if_accepted" | "prohibited_if_accepted") {
        return other.is_some_and(|v| in_values(v, "yes,on,1,true"));
    }
    if matches!(rule, "required_if_declined" | "prohibited_if_declined") {
        return other.is_some_and(|v| in_values(v, "no,off,0,false"));
    }
    if matches!(rule, "accepted_if" | "declined_if") {
        return other.is_some_and(|v| values.contains(&v));
    }
    if rule.ends_with("_if") {
        return other.is_some_and(|v| values.contains(&v));
    }
    if rule.ends_with("_unless") {
        return other.is_none_or(|v| !values.contains(&v));
    }
    match rule {
        "present_with" | "missing_with" => args
            .iter()
            .any(|name| *name != field && request.exists(name)),
        "exclude_with" | "required_with" => args
            .iter()
            .any(|name| *name != field && request.filled(name)),
        "exclude_without" | "required_without" | "present_without" | "missing_without" => args
            .iter()
            .any(|name| *name != field && !request.filled(name)),
        "present_with_all" | "missing_with_all" => args
            .iter()
            .all(|name| *name != field && request.exists(name)),
        "required_with_all" => args
            .iter()
            .all(|name| *name != field && request.filled(name)),
        "required_without_all" | "missing_without_all" => args
            .iter()
            .all(|name| *name != field && !request.filled(name)),
        "exclude" | "required" | "present" | "missing" | "filled" | "prohibited" | "prohibits" => {
            true
        }
        _ => false,
    }
}
fn conditional_rule_passes(request: &Request, field: &str, rule: &str, argument: &str) -> bool {
    let present = request.exists(field);
    let filled = request.filled(field);
    let condition = matches_condition(request, field, rule, argument);
    match rule {
        "required" => filled,
        "present" => present,
        "missing" => !present,
        "filled" => !present || filled,
        "required_if"
        | "required_if_accepted"
        | "required_if_declined"
        | "required_unless"
        | "required_with"
        | "required_with_all"
        | "required_without"
        | "required_without_all" => !condition || filled,
        "accepted_if" => {
            !condition
                || request
                    .input(field)
                    .is_some_and(|value| in_values(value, "yes,on,1,true"))
        }
        "declined_if" => {
            !condition
                || request
                    .input(field)
                    .is_some_and(|value| in_values(value, "no,off,0,false"))
        }
        "present_if" | "present_unless" | "present_with" | "present_with_all"
        | "present_without" => !condition || present,
        "missing_if"
        | "missing_unless"
        | "missing_with"
        | "missing_with_all"
        | "missing_without"
        | "missing_without_all" => !condition || !present,
        "prohibited" => !filled,
        "prohibited_if"
        | "prohibited_if_accepted"
        | "prohibited_if_declined"
        | "prohibited_unless" => !condition || !filled,
        "prohibits" => {
            !filled
                || split(argument, ',')
                    .iter()
                    .all(|name| *name == field || !request.filled(name))
        }
        _ => true,
    }
}
fn excludes_field(request: &Request, field: &str, rule: &str, argument: &str) -> bool {
    rule == "exclude"
        || (matches!(
            rule,
            "exclude_if" | "exclude_unless" | "exclude_with" | "exclude_without"
        ) && matches_condition(request, field, rule, argument))
}

fn accepts(
    request: &Request,
    field: &str,
    value: &str,
    rule: &str,
    argument: &str,
    numeric_attribute: bool,
) -> bool {
    use std::net::IpAddr;
    let other = request.input(argument);
    let file = request.file(field);
    match rule {
        "required" | "nullable" => !value.is_empty(),
        "string" => !request.is_array(field) && !request.is_object(field),
        "array" => {
            (request.is_array(field) || request.is_object(field))
                && (argument.is_empty()
                    || request
                        .array_keys(field)
                        .iter()
                        .all(|key| in_values(key, argument)))
        }
        "list" => request.is_array(field) && request.array_keys(field).is_empty(),
        "integer" => parse_integer(value),
        "numeric" => parse_number(value).is_some(),
        "boolean" => {
            if argument == "strict" {
                matches!(value, "true" | "false")
            } else {
                matches!(value, "0" | "1" | "true" | "false")
            }
        }
        "accepted" => in_values(value, "yes,on,1,true"),
        "declined" => in_values(value, "no,off,0,false"),
        "email" => valid_email(value, argument),
        "file" => file.is_some(),
        "image" => file.is_some_and(|file| {
            detected_mime(&file.contents).starts_with("image/")
                && (detected_mime(&file.contents) != "image/svg+xml"
                    || in_values("allow_svg", argument))
        }),
        "mimes" => file.is_some_and(|file| {
            let mime = detected_mime(&file.contents);
            split(argument, ',')
                .iter()
                .any(|ext| mime_for_extension(ext) == Some(mime.as_str()))
        }),
        "mimetypes" => file.is_some_and(|file| in_values(&detected_mime(&file.contents), argument)),
        "extensions" => file.is_some_and(|file| {
            in_values(
                file.name
                    .rsplit_once('.')
                    .map_or("", |(_, ext)| ext)
                    .to_ascii_lowercase()
                    .as_str(),
                argument,
            )
        }),
        "encoding" => valid_encoding(
            file.map_or(value.as_bytes(), |file| file.contents.as_slice()),
            argument,
        ),
        "dimensions" => file.is_some_and(|file| dimensions_match(&file.contents, argument)),
        "url" => valid_url(value),
        "active_url" => valid_url(value) && url_host(value).is_some_and(|host| resolve_host(&host)),
        "ip" => value.parse::<IpAddr>().is_ok(),
        "ipv4" => value.parse::<std::net::Ipv4Addr>().is_ok(),
        "ipv6" => value.parse::<std::net::Ipv6Addr>().is_ok(),
        "json" => serde_json::from_str::<serde_json::Value>(value).is_ok(),
        "ascii" => value.is_ascii(),
        "alpha" => valid_alpha(value, argument, false, false),
        "alpha_num" => valid_alpha(value, argument, true, false),
        "alpha_dash" => valid_alpha(value, argument, true, true),
        "lowercase" => value
            .chars()
            .all(|ch| !ch.is_alphabetic() || ch.is_lowercase()),
        "uppercase" => value
            .chars()
            .all(|ch| !ch.is_alphabetic() || ch.is_uppercase()),
        "in" | "enum" => in_values(value, argument),
        "not_in" => !in_values(value, argument),
        "any_of" => split(argument, ';').iter().any(|candidate| {
            let (rule, arg) = rule_parts(candidate);
            accepts(request, field, value, rule, arg, numeric_attribute)
        }),
        "same" => other.is_some_and(|other| value == other),
        "different" => other.is_none_or(|other| value != other),
        "confirmed" => {
            let confirmation = if argument.is_empty() {
                format!("{field}_confirmation")
            } else {
                argument.to_owned()
            };
            request
                .input(&confirmation)
                .is_some_and(|confirm| confirm == value)
        }
        "starts_with" => split(argument, ',')
            .iter()
            .any(|part| value.starts_with(part)),
        "ends_with" => split(argument, ',')
            .iter()
            .any(|part| value.ends_with(part)),
        "doesnt_start_with" => split(argument, ',')
            .iter()
            .all(|part| !value.starts_with(part)),
        "doesnt_end_with" => split(argument, ',')
            .iter()
            .all(|part| !value.ends_with(part)),
        "regex" => validate_regex(value, argument),
        "not_regex" => !validate_regex(value, argument),
        "hex_color" => validate_regex(
            value,
            r"#?([0-9a-fA-F]{3}|[0-9a-fA-F]{4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})",
        ),
        "mac_address" => validate_regex(value, r"([0-9a-fA-F]{2}[:-]){5}[0-9a-fA-F]{2}"),
        "uuid" => validate_regex(
            value,
            r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}",
        ),
        "ulid" => validate_regex(value, r"[0-7][0-9A-HJKMNP-TV-Z]{25}"),
        "date" => valid_date(value),
        "date_format" => split(argument, ',')
            .iter()
            .any(|format| valid_date_format(value, format)),
        "date_equals" | "after" | "after_or_equal" | "before" | "before_or_equal" => {
            let comparable = other.unwrap_or(argument);
            if !valid_date(value) || !valid_date(comparable) {
                return false;
            }
            match rule {
                "date_equals" => value == comparable,
                "after" => value > comparable,
                "after_or_equal" => value >= comparable,
                "before" => value < comparable,
                _ => value <= comparable,
            }
        }
        "timezone" => valid_timezone(value, argument),
        "min" | "max" | "size" | "between" => size_rule(
            request,
            field,
            value,
            rule,
            argument,
            numeric_attribute,
            file.map(|f| f.contents.len()),
        ),
        "decimal" => decimal_rule(value, argument),
        "digits" | "digits_between" | "min_digits" | "max_digits" => {
            digits_rule(value, argument, rule)
        }
        "gt" | "gte" | "lt" | "lte" | "multiple_of" => {
            comparison_rule(value, argument, rule, other)
        }
        "array_keys" => {
            let keys = request.array_keys(field);
            !keys.is_empty() && keys.iter().all(|key| in_values(key, argument))
        }
        "required_array_keys" => split(argument, ',')
            .iter()
            .all(|key| request.array_keys(field).contains(&key.to_string())),
        "contains" => split(argument, ',').iter().all(|wanted| {
            request
                .array_values(field)
                .iter()
                .any(|value| value == wanted)
        }),
        "doesnt_contain" => split(argument, ',').iter().all(|unwanted| {
            request
                .array_values(field)
                .iter()
                .all(|value| value != unwanted)
        }),
        "distinct" => {
            let mut values = request.array_values(field).to_vec();
            if argument.contains("ignore_case") {
                values.iter_mut().for_each(|v| *v = v.to_ascii_lowercase());
            }
            values.sort();
            values.windows(2).all(|w| w[0] != w[1])
        }
        "in_array" => {
            if let Some((array, _)) = argument.split_once(".*") {
                request.array_values(array).iter().any(|item| item == value)
            } else {
                request
                    .array_values(argument)
                    .iter()
                    .any(|item| item == value)
                    || other.is_some_and(|item| item == value)
            }
        }
        "in_array_keys" => split(argument, ',')
            .iter()
            .any(|key| request.array_keys(field).contains(&key.to_string())),
        _ => true,
    }
}

fn valid_email(value: &str, argument: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    let syntax = !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.contains("..")
        && !value.chars().any(char::is_whitespace);
    if !syntax {
        return false;
    }
    let styles = split(argument, ',');
    styles
        .iter()
        .all(|s| matches!(*s, "rfc" | "strict" | "filter" | "dns"))
        && (!styles.contains(&"dns") || resolve_host(domain))
}
fn valid_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| !url.scheme().is_empty() && url.host_str().is_some())
}
fn url_host(value: &str) -> Option<String> {
    Some(Url::parse(value).ok()?.host_str()?.to_owned())
}
fn resolve_host(host: &str) -> bool {
    (host, 80)
        .to_socket_addrs()
        .is_ok_and(|mut addresses| addresses.next().is_some())
}
fn valid_alpha(value: &str, argument: &str, numeric: bool, dash: bool) -> bool {
    if value.is_empty() {
        return false;
    }
    let ascii = argument == "ascii";
    value.chars().all(|ch| {
        ch.is_alphabetic()
            || (!ascii && !ch.is_ascii())
            || (numeric && ch.is_ascii_digit())
            || (dash && (ch == '_' || ch == '-'))
    })
}
fn detected_mime(bytes: &[u8]) -> String {
    let trimmed = match bytes.iter().position(|byte| !byte.is_ascii_whitespace()) {
        Some(index) => &bytes[index..],
        None => bytes,
    };
    let sample = String::from_utf8_lossy(&trimmed[..trimmed.len().min(512)]).to_ascii_lowercase();
    if sample.starts_with("<svg") || (sample.starts_with("<?xml") && sample.contains("<svg")) {
        "image/svg+xml"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"PK\x03\x04") {
        "application/zip"
    } else if !bytes.contains(&0) {
        "text/plain"
    } else {
        "application/octet-stream"
    }
    .into()
}
fn mime_for_extension(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "csv" => "text/plain",
        "zip" => "application/zip",
        "json" => "application/json",
        _ => return None,
    })
}
fn dimensions_match(bytes: &[u8], argument: &str) -> bool {
    let Some((width, height)) = image_dimensions(bytes) else {
        return false;
    };
    split(argument, ',').iter().all(|constraint| {
        let Some((name, expected)) = constraint.split_once('=') else {
            return false;
        };
        if name == "ratio" {
            let mut ratio = expected.split('/');
            let n = ratio.next().and_then(parse_number);
            let d = match ratio.next() {
                Some(value) => parse_number(value),
                None => Some(1.0),
            };
            return n.zip(d).is_some_and(|(n, d)| {
                d != 0.0 && ((width as f64 / height as f64) - n / d).abs() <= 0.01
            });
        }
        let Some(expected) = parse_number(expected) else {
            return false;
        };
        match name {
            "width" => width as f64 == expected,
            "min_width" => width as f64 >= expected,
            "max_width" => width as f64 <= expected,
            "height" => height as f64 == expected,
            "min_height" => height as f64 >= expected,
            "max_height" => height as f64 <= expected,
            _ => false,
        }
    })
}
fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        return Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ));
    }
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        return Some((
            u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
        ));
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        return Some((
            u32::from_le_bytes(bytes[18..22].try_into().ok()?),
            u32::from_le_bytes(bytes[22..26].try_into().ok()?),
        ));
    }
    None
}
fn valid_encoding(bytes: &[u8], name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    let encoding: Option<&'static Encoding> = match normalized.as_str() {
        "utf-8" | "utf8" => Some(UTF_8),
        "ascii" | "us-ascii" => return bytes.is_ascii(),
        "iso-8859-1" | "latin1" | "windows-1252" | "cp1252" => Some(WINDOWS_1252),
        "utf-16le" => Some(UTF_16LE),
        "utf-16be" => Some(UTF_16BE),
        _ => None,
    };
    encoding.is_some_and(|encoding| {
        let (_, had_errors) = encoding.decode_without_bom_handling(bytes);
        !had_errors
    })
}
fn valid_date(value: &str) -> bool {
    let date = value.get(..10).filter(|_| value.len() >= 10);
    let Some(date) = date else {
        return false;
    };
    let Ok(_) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return false;
    };
    if value.len() == 10 {
        return true;
    }
    if value.len() < 19
        || !matches!(value.as_bytes()[10], b'T' | b' ')
        || value.as_bytes()[13] != b':'
        || value.as_bytes()[16] != b':'
    {
        return false;
    }
    let Ok(_) = chrono::NaiveTime::parse_from_str(&value[11..19], "%H:%M:%S") else {
        return false;
    };
    value[19..]
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '+' | '-' | ':' | 'Z' | 'z'))
}
fn valid_date_format(value: &str, format: &str) -> bool {
    let mut regex = String::from("^");
    let mut fields = Vec::new();
    let mut chars = format.chars();
    while let Some(token) = chars.next() {
        if token == '\\' {
            if let Some(literal) = chars.next() {
                regex.push_str(&regex::escape(&literal.to_string()));
            }
            continue;
        }
        let (pattern, capture) = match token {
            'Y' => ("[0-9]{4}", Some('Y')),
            'y' => ("[0-9]{2}", Some('y')),
            'm' | 'd' | 'H' | 'h' | 'i' | 's' => ("[0-9]{2}", Some(token)),
            'n' | 'j' | 'G' | 'g' => ("[0-9]{1,2}", Some(token)),
            _ => {
                regex.push_str(&regex::escape(&token.to_string()));
                continue;
            }
        };
        regex.push('(');
        regex.push_str(pattern);
        regex.push(')');
        fields.push(capture.unwrap());
    }
    regex.push('$');
    let Ok(regex) = Regex::new(&regex) else {
        return false;
    };
    let Some(captures) = regex.captures(value) else {
        return false;
    };
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) = (2000, 1, 1, 0, 0, 0);
    for (i, field) in fields.iter().enumerate() {
        let Ok(value) = captures[i + 1].parse::<u32>() else {
            return false;
        };
        match field {
            'Y' => year = value as i32,
            'y' => {
                year = if value < 70 {
                    2000 + value as i32
                } else {
                    1900 + value as i32
                }
            }
            'm' | 'n' => month = value,
            'd' | 'j' => day = value,
            'H' | 'h' | 'G' | 'g' => hour = value,
            'i' => minute = value,
            's' => second = value,
            _ => (),
        }
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day).is_some()
        && hour < 24
        && minute < 60
        && second < 60
}
fn valid_timezone(value: &str, argument: &str) -> bool {
    let args = split(argument, ',');
    let tz = Tz::from_str(value).is_ok();
    if !tz {
        return false;
    }
    if args.is_empty() || args[0] == "all" {
        return true;
    }
    if args[0] == "per_country" {
        return args
            .get(1)
            .is_some_and(|country| timezone_country_matches(value, country));
    }
    value.starts_with(&format!("{}/", args[0]))
}
fn timezone_country_matches(value: &str, country: &str) -> bool {
    if country.len() != 2 {
        return false;
    }
    let prefix = match country {
        "US" => "America/",
        "CA" => "America/",
        "MX" => "America/",
        "GB" => "Europe/",
        "FR" => "Europe/",
        "DE" => "Europe/",
        "AU" => "Australia/",
        "JP" => "Asia/",
        "IN" => "Asia/",
        _ => return false,
    };
    value.starts_with(prefix)
}
fn size_rule(
    request: &Request,
    field: &str,
    value: &str,
    rule: &str,
    argument: &str,
    numeric: bool,
    file_size: Option<usize>,
) -> bool {
    let bounds = split(argument, ',');
    if bounds.is_empty() || (rule == "between" && bounds.len() != 2) {
        return false;
    }
    let Some(lower) = parse_number(bounds[0]) else {
        return false;
    };
    let upper = bounds.get(1).and_then(|value| parse_number(value));
    let size = if let Some(bytes) = file_size {
        bytes as f64 / 1024.0
    } else if request.is_array(field) {
        request.array_values(field).len() as f64
    } else if request.is_object(field) {
        request.array_keys(field).len() as f64
    } else if numeric {
        let Some(value) = parse_number(value) else {
            return false;
        };
        value
    } else {
        chars(value) as f64
    };
    match rule {
        "min" => size >= lower,
        "max" => size <= lower,
        "size" => size == lower,
        "between" => upper.is_some_and(|upper| size >= lower && size <= upper),
        _ => false,
    }
}
fn decimal_rule(value: &str, argument: &str) -> bool {
    if parse_number(value).is_none() {
        return false;
    }
    let decimals = value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let bounds = split(argument, ',');
    let Some(min) = bounds.first().and_then(|v| parse_number(v)) else {
        return false;
    };
    let max = bounds.get(1).and_then(|v| parse_number(v)).unwrap_or(min);
    decimals as f64 >= min && decimals as f64 <= max
}
fn digits_rule(value: &str, argument: &str, rule: &str) -> bool {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let bounds = split(argument, ',');
    let Some(first) = bounds.first().and_then(|v| parse_number(v)) else {
        return false;
    };
    let len = value.len() as f64;
    match rule {
        "digits" => len == first,
        "min_digits" => len >= first,
        "max_digits" => len <= first,
        _ => bounds
            .get(1)
            .and_then(|v| parse_number(v))
            .is_some_and(|last| len >= first && len <= last),
    }
}
fn comparison_rule(value: &str, argument: &str, rule: &str, other: Option<&str>) -> bool {
    let Some(left) = parse_number(value) else {
        return false;
    };
    let Some(right) = parse_number(argument).or_else(|| other.and_then(parse_number)) else {
        return false;
    };
    match rule {
        "gt" => left > right,
        "gte" => left >= right,
        "lt" => left < right,
        "lte" => left <= right,
        _ => right != 0.0 && (left / right).fract() == 0.0,
    }
}

fn validation_error(field: &str, rule: &str, argument: &str) -> ValidationError {
    let label = field;
    let detail = match rule {
        "required" => format!("The {label} field is required."),
        "string" => format!("The {label} field must be a string."),
        "integer" => format!("The {label} field must be an integer."),
        "numeric" => format!("The {label} field must be a number."),
        "boolean" => format!("The {label} field must be true or false."),
        "email" => format!("The {label} field must be a valid email address."),
        "min" => format!("The {label} field must be at least {argument}."),
        "max" => format!("The {label} field may not be greater than {argument}."),
        "in" => format!("The selected {label} is invalid."),
        _ => format!("The {label} field is invalid."),
    };
    ValidationError {
        title: "Validation Failed".into(),
        detail,
        field: Some(field.into()),
    }
}
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
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
}
