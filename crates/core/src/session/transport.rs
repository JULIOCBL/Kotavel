//! Cookie and header adapters for carrying opaque session identifiers.

use super::valid_session_id;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionIdentifierTransport {
    Cookie(CookieTransportOptions),
    Header { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookieTransportOptions {
    pub name: String,
    pub path: String,
    pub domain: Option<String>,
    pub max_age_seconds: Option<u64>,
    pub http_only: bool,
    pub secure: bool,
    pub same_site: SameSite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Lax,
    Strict,
    None,
}

impl Default for CookieTransportOptions {
    fn default() -> Self {
        Self {
            name: "kotavel_session".into(),
            path: "/".into(),
            domain: None,
            max_age_seconds: Some(7200),
            http_only: true,
            secure: false,
            same_site: SameSite::Lax,
        }
    }
}

impl SessionIdentifierTransport {
    pub fn cookie(options: CookieTransportOptions) -> Result<Self, &'static str> {
        if options.name.is_empty() || !options.name.bytes().all(is_token_char) {
            return Err("session cookie name is invalid");
        }
        if !options.path.starts_with('/')
            || options
                .path
                .bytes()
                .any(|b| b <= 0x20 || b >= 0x7f || b == b';' || b == b',')
        {
            return Err("session cookie path must be an absolute safe path");
        }
        if options.domain.as_ref().is_some_and(|domain| {
            domain.is_empty()
                || domain
                    .bytes()
                    .any(|b| !(b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
        }) {
            return Err("session cookie domain is invalid");
        }
        if options.same_site == SameSite::None && !options.secure {
            return Err("SameSite=None cookies require Secure=true");
        }
        Ok(Self::Cookie(options))
    }

    pub fn header(name: impl Into<String>) -> Result<Self, &'static str> {
        let name = name.into();
        if name.is_empty() || !name.bytes().all(is_token_char) {
            return Err("session identifier header name is invalid");
        }
        Ok(Self::Header { name })
    }

    pub fn is_cookie(&self) -> bool {
        matches!(self, Self::Cookie(_))
    }

    pub fn input_name(&self) -> &str {
        match self {
            Self::Cookie(options) => &options.name,
            Self::Header { name } => name,
        }
    }

    /// Read an identifier from the selected transport. Invalid IDs are ignored
    /// so the session manager creates a fresh, independent session.
    pub fn read(&self, cookie_header: Option<&str>, header_value: Option<&str>) -> Option<String> {
        let candidate = match self {
            Self::Cookie(options) => read_cookie(cookie_header?, &options.name)?,
            Self::Header { .. } => header_value?,
        };
        valid_session_id(candidate).then(|| candidate.to_owned())
    }

    /// Build the response header without accepting a caller-controlled ID.
    pub fn response_header(&self, id: &str) -> Result<(String, String), &'static str> {
        if !valid_session_id(id) {
            return Err("invalid session identifier");
        }
        match self {
            Self::Header { name } => Ok((name.clone(), id.to_owned())),
            Self::Cookie(options) => {
                let mut value = format!("{}={}; Path={}", options.name, id, options.path);
                if let Some(domain) = &options.domain {
                    value.push_str(&format!("; Domain={domain}"));
                }
                if let Some(max_age) = options.max_age_seconds {
                    value.push_str(&format!("; Max-Age={max_age}"));
                }
                if options.http_only {
                    value.push_str("; HttpOnly");
                }
                if options.secure {
                    value.push_str("; Secure");
                }
                value.push_str(match options.same_site {
                    SameSite::Lax => "; SameSite=Lax",
                    SameSite::Strict => "; SameSite=Strict",
                    SameSite::None => "; SameSite=None",
                });
                Ok(("Set-Cookie".into(), value))
            }
        }
    }
}

impl crate::session::SessionIdentifierTransportContract for SessionIdentifierTransport {
    fn read(&self, cookie_header: Option<&str>, header_value: Option<&str>) -> Option<String> {
        SessionIdentifierTransport::read(self, cookie_header, header_value)
    }

    fn response_header(&self, id: &str) -> Result<(String, String), &'static str> {
        SessionIdentifierTransport::response_header(self, id)
    }
}

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn read_cookie<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name).then_some(value.trim())
    })
}

#[cfg(test)]
#[path = "../../tests/unit/session/transport.rs"]
mod tests;
