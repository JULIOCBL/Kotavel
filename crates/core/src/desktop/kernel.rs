use super::{DesktopError, DesktopRouter, DispatchResult, WindowManager, valid_route};
use crate::session::SessionContext;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Platform adapter contract. A platform owns the native UI event loop and
/// calls the supplied callbacks on its UI thread.
pub trait DesktopPlatform: Send {
    fn run(
        &mut self,
        arguments: &[String],
        views_root: &Path,
        on_start: &mut dyn FnMut(&mut dyn WindowManager),
        on_deep_link: &mut dyn FnMut(&str, &mut dyn WindowManager),
    ) -> Result<i32, DesktopError>;
}

pub struct DesktopKernel {
    platform: Box<dyn DesktopPlatform>,
    views_root: PathBuf,
    scheme: String,
    routes: DesktopRouter,
    session_context: Option<Arc<SessionContext>>,
}

impl DesktopKernel {
    pub fn new(platform: impl DesktopPlatform + 'static) -> Self {
        let views_root = std::env::current_exe()
            .ok()
            .and_then(|executable| crate::system_paths::packaged_resources_root(&executable))
            .map(|resources| resources.join("resources/views"))
            .unwrap_or_else(|| PathBuf::from("resources/views"));
        Self::with_options(platform, views_root, "kotavel")
    }

    pub fn with_options(
        platform: impl DesktopPlatform + 'static,
        views_root: impl Into<PathBuf>,
        scheme: impl Into<String>,
    ) -> Self {
        Self {
            platform: Box::new(platform),
            views_root: views_root.into(),
            scheme: scheme.into(),
            routes: DesktopRouter::new(),
            session_context: None,
        }
    }

    /// Attaches shared operational data explicitly; no session identity is shared.
    pub fn with_session_context(mut self, context: Arc<SessionContext>) -> Self {
        self.session_context = Some(context);
        self
    }

    pub fn routes(&mut self) -> &mut DesktopRouter {
        &mut self.routes
    }

    /// Dispatches `/` by default, or the registered route named by a startup
    /// deep link. Kotavel-specific runtime flags are withheld from the UI host.
    pub fn run(&mut self, arguments: &[String]) -> Result<i32, DesktopError> {
        if !self.routes.contains("/") {
            return Err(DesktopError::new(
                "desktop startup route '/' is not registered",
            ));
        }
        let scheme_prefix = format!("{}://", self.scheme);
        let mut startup_route = "/".to_owned();
        let mut platform_arguments = Vec::with_capacity(arguments.len());
        for argument in arguments {
            if argument.starts_with(&scheme_prefix) || argument == &format!("{}:", self.scheme) {
                startup_route = deep_link_path(argument, &self.scheme)?;
            } else if argument.starts_with("--env=")
                || argument.starts_with("--project-root=")
                || argument == "--development"
            {
                continue;
            } else {
                platform_arguments.push(argument.clone());
            }
        }
        if !self.routes.contains(&startup_route) {
            return Err(DesktopError::new(format!(
                "desktop startup route is not registered: {startup_route}"
            )));
        }

        let routes = &self.routes;
        let route = startup_route.clone();
        let session_context = self.session_context.clone();
        let mut start = move |windows: &mut dyn WindowManager| {
            let _ = routes.dispatch_with_session_context(&route, windows, session_context.clone());
        };
        let routes = &self.routes;
        let scheme = &self.scheme;
        let session_context = self.session_context.clone();
        let mut deep_link = move |uri: &str, windows: &mut dyn WindowManager| {
            if let Ok(path) = deep_link_path(uri, scheme) {
                let _ =
                    routes.dispatch_with_session_context(&path, windows, session_context.clone());
            }
        };
        self.platform.run(
            &platform_arguments,
            &self.views_root,
            &mut start,
            &mut deep_link,
        )
    }
}

pub fn dispatch_deep_link(
    router: &DesktopRouter,
    uri: &str,
    scheme: &str,
    windows: &mut dyn WindowManager,
) -> DispatchResult {
    match deep_link_path(uri, scheme) {
        Ok(path) => router.dispatch(&path, windows),
        Err(_) => DispatchResult::InvalidPath,
    }
}

fn deep_link_path(uri: &str, scheme: &str) -> Result<String, DesktopError> {
    let prefix = format!("{scheme}://");
    if uri == format!("{scheme}:")
        || uri == format!("{scheme}:/")
        || uri == prefix
        || uri == format!("{prefix}/")
    {
        return Ok("/".into());
    }
    let Some(destination) = uri.strip_prefix(&prefix) else {
        return Err(DesktopError::new("desktop deep link scheme is invalid"));
    };
    if destination.contains(['?', '#', '@', ':']) {
        return Err(DesktopError::new("desktop deep link is malformed"));
    }
    let (host, suffix) = destination
        .split_once('/')
        .map_or((destination, ""), |(host, suffix)| (host, suffix));
    if host.is_empty() || !super::valid_segmented_name(host, false) {
        return Err(DesktopError::new(
            "desktop deep link destination is invalid",
        ));
    }
    let path = if suffix.is_empty() {
        format!("/{host}")
    } else {
        format!("/{host}/{suffix}")
    };
    if !valid_route(&path) {
        return Err(DesktopError::new("desktop deep link route is invalid"));
    }
    Ok(path)
}
