/// Port for transporting an opaque session identifier over a request/response.
/// HTTP adapters translate their request and response types at this boundary.
pub trait SessionIdentifierTransport: Send + Sync {
    fn read(&self, cookie_header: Option<&str>, header_value: Option<&str>) -> Option<String>;
    fn response_header(&self, id: &str) -> Result<(String, String), &'static str>;
}
