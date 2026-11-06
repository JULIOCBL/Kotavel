use super::{
    ApiResponse, Middleware, Request, Response, RouteDefinition, RouteKind, Router, RouterError,
    Server, ServerError, ServerSettings, ServerStartedCallback,
};
use chrono::{DateTime, SecondsFormat, Utc};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{
    io::Write,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub type ExceptionReporter = dyn Fn(&RouteDefinition, &Request, &str) + Send + Sync + 'static;

#[derive(Default)]
pub struct MiddlewareGroups {
    pub api: Vec<Middleware>,
    pub web: Vec<Middleware>,
}

#[derive(Default)]
pub struct MiddlewareConfiguration {
    pub middleware: Vec<Middleware>,
    pub middleware_groups: MiddlewareGroups,
    pub route_middleware: Vec<(String, Middleware)>,
}

/// Middleware and route execution shared by any replaceable HTTP server.
#[derive(Clone)]
pub struct HttpKernel {
    routes: Router,
    global_middleware: Vec<Middleware>,
    api_middleware: Vec<Middleware>,
    web_middleware: Vec<Middleware>,
    exception_reporter: Option<Arc<ExceptionReporter>>,
    started: bool,
}

impl Default for HttpKernel {
    fn default() -> Self {
        Self {
            routes: Router::new(),
            global_middleware: Vec::new(),
            api_middleware: Vec::new(),
            web_middleware: Vec::new(),
            exception_reporter: None,
            started: false,
        }
    }
}

impl HttpKernel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn routes(&self) -> &Router {
        &self.routes
    }
    pub fn routes_mut(&mut self) -> Result<&mut Router, RouterError> {
        if self.started {
            return Err(RouterError(
                "HTTP routes are sealed after the kernel starts".into(),
            ));
        }
        Ok(&mut self.routes)
    }
    pub fn use_global(&mut self, middleware: Middleware) -> Result<(), RouterError> {
        self.ensure_configurable()?;
        self.global_middleware.push(middleware);
        Ok(())
    }
    pub fn use_api(&mut self, middleware: Middleware) -> Result<(), RouterError> {
        self.ensure_configurable()?;
        self.api_middleware.push(middleware);
        Ok(())
    }
    pub fn use_web(&mut self, middleware: Middleware) -> Result<(), RouterError> {
        self.ensure_configurable()?;
        self.web_middleware.push(middleware);
        Ok(())
    }
    pub fn alias_middleware(
        &mut self,
        alias: &str,
        middleware: Middleware,
    ) -> Result<(), RouterError> {
        self.ensure_configurable()?;
        self.routes.alias_middleware(alias, middleware)
    }
    pub fn configure_middleware(
        &mut self,
        configuration: MiddlewareConfiguration,
    ) -> Result<(), RouterError> {
        self.ensure_configurable()?;
        for (index, (alias, _)) in configuration.route_middleware.iter().enumerate() {
            if alias.is_empty() {
                return Err(RouterError(
                    "HTTP route middleware alias cannot be empty".into(),
                ));
            }
            if configuration.route_middleware[..index]
                .iter()
                .any(|(prior, _)| prior == alias)
            {
                return Err(RouterError(format!(
                    "Duplicate HTTP middleware alias: {alias}"
                )));
            }
            if self.routes.contains_middleware_alias(alias) {
                return Err(RouterError(format!(
                    "Duplicate HTTP middleware alias: {alias}"
                )));
            }
        }
        // Validate duplicate aliases against previously configured aliases before mutation.
        for (alias, middleware) in &configuration.route_middleware {
            self.routes.alias_middleware(alias, middleware.clone())?;
        }
        self.global_middleware.extend(configuration.middleware);
        self.api_middleware
            .extend(configuration.middleware_groups.api);
        self.web_middleware
            .extend(configuration.middleware_groups.web);
        Ok(())
    }
    pub fn set_exception_reporter<F>(&mut self, reporter: F) -> Result<(), RouterError>
    where
        F: Fn(&RouteDefinition, &Request, &str) + Send + Sync + 'static,
    {
        self.ensure_configurable()?;
        self.exception_reporter = Some(Arc::new(reporter));
        Ok(())
    }
    pub fn seal(&mut self) {
        self.routes.seal();
        self.started = true;
    }
    pub fn started(&self) -> bool {
        self.started
    }

    /// Prints one access-log line for a completed HTTP transport request.
    pub fn log_access(
        method: &str,
        path: &str,
        status: u16,
        received_at: SystemTime,
        elapsed: Duration,
    ) {
        let line = format_access_log(method, path, status, received_at, elapsed);
        let stdout = std::io::stdout();
        let _ = writeln!(stdout.lock(), "{line}");
    }

    /// Seals configuration and transfers an immutable kernel snapshot to the
    /// selected transport. A kernel instance can only be started once.
    pub async fn run(
        &mut self,
        server: &dyn Server,
        settings: ServerSettings,
        started: Option<ServerStartedCallback>,
    ) -> Result<(), ServerError> {
        settings.validate()?;
        if self.started {
            return Err(Box::new(std::io::Error::other(
                "HTTP kernel can only be started once",
            )));
        }
        self.seal();
        server.run(Arc::new(self.clone()), settings, started).await
    }

    /// Requests transport shutdown. The server owns the platform-specific
    /// mechanism for stopping its listener and worker tasks.
    pub fn stop(&self, server: &dyn Server) {
        server.stop();
    }

    pub fn handle(&self, route: &RouteDefinition, request: &Request) -> Response {
        if !request_accepts_route(request, route.kind) {
            return not_acceptable(route.kind);
        }
        if !request.has_valid_input() {
            return invalid_json_request(route.kind);
        }

        let group = if route.kind == RouteKind::Api {
            &self.api_middleware
        } else {
            &self.web_middleware
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            invoke_chain(request, route, &self.global_middleware, group, 0)
        }));
        match result {
            Ok(mut response) => {
                if !(100..=599).contains(&response.status) {
                    response.status = 500;
                }
                response
            }
            Err(payload) => {
                let message = panic_message(payload);
                if let Some(reporter) = &self.exception_reporter {
                    let _ = catch_unwind(AssertUnwindSafe(|| reporter(route, request, &message)));
                }
                internal_server_error(route.kind)
            }
        }
    }

    pub fn not_found(&self, request: &Request) -> Response {
        let kind = if request.path() == "/api" || request.path().starts_with("/api/") {
            RouteKind::Api
        } else {
            RouteKind::Web
        };
        if !request_accepts_route(request, kind) {
            return not_acceptable(kind);
        }
        match kind {
            RouteKind::Api => api_error(404, "Not Found", "The requested API route was not found."),
            RouteKind::Web => html_error(404, "404 Not Found"),
        }
    }

    pub fn method_not_allowed(&self, request: &Request) -> Response {
        let kind = request.kind();
        if !request_accepts_route(request, kind) {
            return not_acceptable(kind);
        }
        let forbidden = request.raw_method().eq_ignore_ascii_case("OPTIONS");
        let status = if forbidden { 403 } else { 405 };
        let (title, detail) = if forbidden {
            (
                "Forbidden",
                "This HTTP method is not permitted for the requested route.",
            )
        } else {
            (
                "Method Not Allowed",
                "The requested HTTP method is not allowed for this route.",
            )
        };
        if kind == RouteKind::Api {
            api_error(status, title, detail)
        } else {
            html_error(status, &format!("{status} {title}"))
        }
    }

    fn ensure_configurable(&self) -> Result<(), RouterError> {
        if self.started {
            Err(RouterError(
                "HTTP middleware is sealed after the kernel starts".into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn format_access_log(
    method: &str,
    path: &str,
    status: u16,
    received_at: SystemTime,
    elapsed: Duration,
) -> String {
    let timestamp = DateTime::<Utc>::from(received_at).to_rfc3339_opts(SecondsFormat::Millis, true);
    let printable_path: String = path
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            other => other,
        })
        .collect();
    format!(
        "[HTTP] {timestamp} {method} {printable_path} {status} {:.3} ms",
        elapsed.as_secs_f64() * 1_000.0
    )
}

fn invoke_chain(
    request: &Request,
    route: &RouteDefinition,
    global: &[Middleware],
    group: &[Middleware],
    position: usize,
) -> Response {
    let middleware = if position < global.len() {
        Some(&global[position])
    } else if position - global.len() < group.len() {
        Some(&group[position - global.len()])
    } else {
        let route_position = position - global.len() - group.len();
        route.middleware.get(route_position)
    };
    let Some(middleware) = middleware else {
        return (route.handler)(request);
    };
    let next = || invoke_chain(request, route, global, group, position + 1);
    middleware(request, &next)
}

fn request_accepts_route(request: &Request, kind: RouteKind) -> bool {
    let offered = if kind == RouteKind::Api {
        "application/json"
    } else {
        "text/html"
    };
    accepts_media_type(request.header("accept").unwrap_or_default(), offered)
}

fn accepts_media_type(accept: &str, offered: &str) -> bool {
    let accept = accept.trim();
    if accept.is_empty() {
        return true;
    }
    let offered = offered
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let Some((offered_major, _)) = offered.split_once('/') else {
        return true;
    };
    let mut best_specificity = -1_i8;
    let mut best_quality = 0.0_f64;
    for member in accept.split(',') {
        let mut parts = member.split(';');
        let range = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
        let mut quality = 1.0;
        for parameter in parts {
            if let Some((name, value)) = parameter.trim().split_once('=') {
                if name.trim().eq_ignore_ascii_case("q") {
                    quality = value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                        .unwrap_or(0.0);
                    break;
                }
            }
        }
        let specificity = if range == offered {
            2
        } else if range == format!("{offered_major}/*") {
            1
        } else if range == "*/*" {
            0
        } else {
            -1
        };
        if specificity > best_specificity {
            best_specificity = specificity;
            best_quality = quality;
        } else if specificity == best_specificity && specificity >= 0 {
            best_quality = best_quality.max(quality);
        }
    }
    best_specificity >= 0 && best_quality > 0.0
}

fn api_error(status: u16, title: &str, detail: &str) -> Response {
    ApiResponse::single_error(status, title, detail).unwrap_or_else(|_| Response::default())
}
fn not_acceptable(kind: RouteKind) -> Response {
    if kind == RouteKind::Api {
        api_error(
            406,
            "Not Acceptable",
            "This route does not provide a representation accepted by the request.",
        )
    } else {
        html_error(406, "406 Not Acceptable")
    }
}
fn invalid_json_request(kind: RouteKind) -> Response {
    if kind == RouteKind::Api {
        api_error(
            400,
            "Bad Request",
            "The request body contains invalid JSON.",
        )
    } else {
        html_error(400, "400 Bad Request")
    }
}
fn internal_server_error(kind: RouteKind) -> Response {
    if kind == RouteKind::Api {
        api_error(
            500,
            "Internal Server Error",
            "An unexpected error occurred while processing the request.",
        )
    } else {
        html_error(500, "500 Internal Server Error")
    }
}
fn html_error(status: u16, title: &str) -> Response {
    Response::new(
        status,
        "text/html; charset=utf-8",
        format!(
            "<!doctype html><html><head><title>{title}</title></head><body><h1>{title}</h1></body></html>"
        ),
    )
}
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "non-string panic payload".into()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/http/kernel.rs"]
mod tests;
