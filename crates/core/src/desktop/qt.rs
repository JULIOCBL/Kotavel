use super::{DesktopError, DesktopPlatform, View, WindowManager, WindowSettings};
use std::path::Path;

mod bridge;

/// Native Qt Quick/QML implementation of the desktop platform contract.
///
/// Enable the `desktop-qt` Cargo feature and install Qt 6 development tools to
/// build this adapter. Applications that do not use Desktop need no Qt install.
#[derive(Default)]
pub struct QtDesktopPlatform;

impl QtDesktopPlatform {
    pub fn new() -> Self {
        Self
    }
}

impl DesktopPlatform for QtDesktopPlatform {
    fn run(
        &mut self,
        arguments: &[String],
        views_root: &Path,
        on_start: &mut dyn FnMut(&mut dyn WindowManager),
        on_deep_link: &mut dyn FnMut(&str, &mut dyn WindowManager),
    ) -> Result<i32, DesktopError> {
        let host = bridge::create_qt_host(arguments.to_vec()).map_err(|error| {
            DesktopError::new(format!("could not create Qt application: {error}"))
        })?;
        let host = host
            .as_ref()
            .ok_or_else(|| DesktopError::new("Qt application could not be created"))?;
        let mut windows = QtWindowManager { host, views_root };
        on_start(&mut windows);
        if !windows.has_visible_window() {
            return Err(DesktopError::new(
                "desktop startup route did not open a visible Qt Quick window",
            ));
        }

        let mut deep_link = bridge::DeepLinkDispatcher {
            callback: on_deep_link,
            windows: &mut windows,
            host,
            callback_failed: false,
        };
        let exit_code = host.exec(&mut deep_link);
        if deep_link.callback_failed {
            return Err(DesktopError::new(
                "desktop deep-link handler panicked; the Qt event loop was stopped",
            ));
        }
        Ok(exit_code)
    }
}

struct QtWindowManager<'a> {
    host: &'a bridge::QtHost,
    views_root: &'a Path,
}

impl WindowManager for QtWindowManager<'_> {
    fn show(
        &mut self,
        id: &str,
        settings: &WindowSettings,
        view: &View,
    ) -> Result<(), DesktopError> {
        if settings.width == 0 || settings.height == 0 {
            return Err(DesktopError::new(
                "desktop window dimensions must be positive",
            ));
        }
        let source = view.resolve(self.views_root)?;
        let source = source
            .to_str()
            .ok_or_else(|| DesktopError::new("desktop view path is not valid UTF-8"))?;
        let icon = self
            .views_root
            .parent()
            .and_then(Path::parent)
            .map(|project_root| project_root.join("assets/branding/kotavel-icon.png"))
            .filter(|path| path.is_file());
        let icon = icon.as_deref().and_then(Path::to_str).unwrap_or_default();
        self.host
            .show(
                id,
                &settings.title,
                settings.width,
                settings.height,
                source,
                icon,
            )
            .map_err(|error| DesktopError::new(error.to_string()))
    }

    fn close(&mut self, id: &str) {
        self.host.close(id);
    }

    fn is_visible(&self, id: &str) -> bool {
        self.host.is_visible(id)
    }

    fn has_visible_window(&self) -> bool {
        self.host.has_visible_window()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{DesktopKernel, Route, WindowSettings, view};

    #[test]
    fn qt_offscreen_platform_loads_qml_and_runs_the_event_loop() {
        let directory = tempfile::tempdir().unwrap();
        let views_root = directory.path().join("views");
        std::fs::create_dir_all(views_root.join("desktop")).unwrap();
        std::fs::write(
            views_root.join("desktop/home.qml"),
            "import QtQuick\nItem { Component.onCompleted: Qt.quit() }\n",
        )
        .unwrap();

        let mut kernel =
            DesktopKernel::with_options(QtDesktopPlatform::new(), &views_root, "kotavel");
        Route::desktop(kernel.routes())
            .get("/", |context| {
                context
                    .show(
                        "main",
                        &WindowSettings::default(),
                        &view("desktop.home").unwrap(),
                    )
                    .unwrap();
            })
            .unwrap();

        assert_eq!(kernel.run(&[]).unwrap(), 0);
    }
}
