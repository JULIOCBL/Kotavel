fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DESKTOP_QT");
    if std::env::var_os("CARGO_FEATURE_DESKTOP_QT").is_none() {
        return;
    }

    cxx_qt_build::CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Qml")
        .qt_module("Quick")
        .qt_module("Network")
        .cpp_file("src/desktop/qt/qt_host.cpp")
        .file("src/desktop/qt/bridge.rs")
        .build();
}
