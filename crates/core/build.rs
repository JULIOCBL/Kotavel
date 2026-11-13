fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DESKTOP_QT");
    #[cfg(feature = "desktop-qt")]
    cxx_qt_build::CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Qml")
        .qt_module("Quick")
        .qt_module("Network")
        .cpp_file("src/desktop/qt/qt_host.cpp")
        .file("src/desktop/qt/bridge.rs")
        .build()
        .export();

    // GNU ld on Windows resolves static references from left to right. Keep
    // these MinGW-specific link-order fixes away from macOS and other targets.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
        && std::env::var_os("CARGO_FEATURE_DESKTOP_QT").is_some()
    {
        // Cargo places CXX-Qt's call-initializer archive after the core rlib.
        // Repeat the generated archive and native runtimes at the end so GNU ld
        // can resolve their references.
        let archive = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("libcore-cxxqt-generated.a");
        println!("cargo:rustc-link-arg={}", archive.display());
        for library in [
            "stdc++",
            "pthread",
            "Qt6Gui",
            "Qt6Qml",
            "Qt6Quick",
            "Qt6Network",
            "Qt6Core",
        ] {
            println!("cargo:rustc-link-arg=-l{library}");
        }
    }
}
