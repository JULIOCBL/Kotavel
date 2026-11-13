use std::path::Path;

/// Emits native link arguments for a consuming CXX-Qt application.
///
/// Call from the consumer's build script when its desktop Qt feature is enabled.
/// The Windows GNU linker needs the core-generated archive and native runtimes
/// repeated after Cargo's static initializer archives.
pub fn configure_desktop_linking() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu")
    {
        return;
    }

    let Some((_, manifest)) = std::env::vars_os().find(|(key, _)| {
        key.to_string_lossy().starts_with("DEP_")
            && key.to_string_lossy().ends_with("_CXX_QT_MANIFEST_PATH")
    }) else {
        panic!("desktop Qt is enabled, but no CXX-Qt dependency manifest was exported");
    };
    let archive = Path::new(&manifest)
        .parent()
        .and_then(Path::parent)
        .expect("CXX-Qt manifest must be inside the core OUT_DIR")
        .join("libcore-cxxqt-generated.a");
    if !archive.is_file() {
        panic!("CXX-Qt generated archive not found: {}", archive.display());
    }

    println!("cargo:rustc-link-arg={}", archive.display());
    println!("cargo:rustc-link-arg=-lstdc++");
    println!("cargo:rustc-link-arg=-lpthread");
}
