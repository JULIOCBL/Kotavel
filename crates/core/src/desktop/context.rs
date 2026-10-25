use super::DesktopRouter;
use crate::session::SessionContext;
use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesktopError(pub(crate) String);

impl DesktopError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl Error for DesktopError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchResult {
    Dispatched,
    InvalidPath,
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    /// Logical identifier such as `desktop.home`.
    pub name: String,
}

impl View {
    /// Safe relative QML path, for example `desktop.pos.home.qml` becomes
    /// `desktop/pos/home.qml` beneath the configured views directory.
    pub fn resource_path(&self) -> Result<PathBuf, DesktopError> {
        view(&self.name)?;
        Ok(PathBuf::from(format!(
            "{}.qml",
            self.name.replace('.', "/")
        )))
    }

    /// Resolves this view beneath an existing views directory and rejects
    /// symlinks that would escape that directory.
    pub fn resolve(&self, views_root: impl AsRef<Path>) -> Result<PathBuf, DesktopError> {
        let root = views_root
            .as_ref()
            .canonicalize()
            .map_err(|_| DesktopError::new("desktop views directory is unavailable"))?;
        if !root.is_dir() {
            return Err(DesktopError::new("desktop views root is not a directory"));
        }
        let candidate = root.join(self.resource_path()?);
        let resolved = candidate
            .canonicalize()
            .map_err(|_| DesktopError::new("desktop view file is unavailable"))?;
        if !resolved.starts_with(&root) || !resolved.is_file() {
            return Err(DesktopError::new(
                "desktop view file escapes its views directory",
            ));
        }
        Ok(resolved)
    }
}

pub fn view(name: impl AsRef<str>) -> Result<View, DesktopError> {
    let name = name.as_ref();
    if !valid_segmented_name(name, false) || !name.starts_with("desktop.") {
        return Err(DesktopError::new(
            "desktop views must use a valid desktop.* identifier",
        ));
    }
    Ok(View { name: name.into() })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowSettings {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            title: "Kotavel".into(),
            width: 1100,
            height: 720,
        }
    }
}

pub trait WindowManager {
    fn show(
        &mut self,
        id: &str,
        settings: &WindowSettings,
        view: &View,
    ) -> Result<(), DesktopError>;
    fn close(&mut self, id: &str);
    fn is_visible(&self, id: &str) -> bool;
    fn has_visible_window(&self) -> bool;
}

pub type Handler = Box<dyn for<'a> Fn(&mut DesktopContext<'a>) + Send + Sync + 'static>;

/// Short-lived access to desktop navigation and windows during a route call.
pub struct DesktopContext<'a> {
    pub(crate) routes: &'a DesktopRouter,
    windows: &'a mut dyn WindowManager,
    session_context: Option<Arc<SessionContext>>,
}

impl<'a> DesktopContext<'a> {
    pub(crate) fn new(
        routes: &'a DesktopRouter,
        windows: &'a mut dyn WindowManager,
        session_context: Option<Arc<SessionContext>>,
    ) -> Self {
        Self {
            routes,
            windows,
            session_context,
        }
    }

    /// Returns the explicitly attached shared operational context, if any.
    pub fn session_context(&self) -> Option<Arc<SessionContext>> {
        self.session_context.clone()
    }

    pub fn show(
        &mut self,
        id: &str,
        settings: &WindowSettings,
        definition: &View,
    ) -> Result<(), DesktopError> {
        view(&definition.name)?;
        if !valid_identifier(id) {
            return Err(DesktopError::new("desktop window id is invalid"));
        }
        if settings.width == 0 || settings.height == 0 {
            return Err(DesktopError::new(
                "desktop window dimensions must be positive",
            ));
        }
        self.windows.show(id, settings, definition)
    }

    pub fn close(&mut self, id: &str) {
        self.windows.close(id);
    }

    pub fn is_visible(&self, id: &str) -> bool {
        self.windows.is_visible(id)
    }

    pub fn navigate(&mut self, path: &str) -> DispatchResult {
        self.routes.dispatch(path, self.windows)
    }
}

pub(crate) fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(crate) fn valid_segmented_name(value: &str, allow_slash: bool) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut previous_separator = true;
    for byte in value.bytes() {
        let separator = byte == b'.' || (allow_slash && byte == b'/');
        if !(separator || byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
            return false;
        }
        if separator && previous_separator {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

pub(crate) fn valid_route(path: &str) -> bool {
    path == "/"
        || (path.starts_with('/') && !path.ends_with('/') && valid_segmented_name(&path[1..], true))
}
