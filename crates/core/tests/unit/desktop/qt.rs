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

    let mut kernel = DesktopKernel::with_options(QtDesktopPlatform::new(), &views_root, "kotavel");
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
