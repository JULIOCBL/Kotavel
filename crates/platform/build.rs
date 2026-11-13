use std::path::Path;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu")
    {
        return;
    }

    for (key, manifest) in std::env::vars_os() {
        if !key.to_string_lossy().starts_with("DEP_")
            || !key.to_string_lossy().ends_with("_CXX_QT_MANIFEST_PATH")
        {
            continue;
        }

        // The core initializer is referenced by CXX-Qt's call-initializer
        // archive. Repeat its archive and native runtimes at the end of this
        // crate's test and binary links for Windows GNU.
        let archive = Path::new(&manifest)
            .parent()
            .and_then(Path::parent)
            .expect("CXX-Qt manifest must be inside the core OUT_DIR")
            .join("libcore-cxxqt-generated.a");
        if !archive.is_file() {
            panic!("CXX-Qt generated archive not found: {}", archive.display());
        }
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
        break;
    }
}
