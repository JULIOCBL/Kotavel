use super::*;

#[test]
fn native_system_paths_return_app_scoped_locations_and_reject_traversal() {
    let paths = NativeSystemPaths;
    let config = paths.config_directory("KotavelPathTest").unwrap();
    let data = paths.data_directory("KotavelPathTest").unwrap();
    let cache = paths.cache_directory("KotavelPathTest").unwrap();
    assert!(config.is_absolute() && data.is_absolute() && cache.is_absolute());
    assert_eq!(config.file_name().unwrap(), "KotavelPathTest");
    assert_eq!(data.file_name().unwrap(), "KotavelPathTest");
    let expected_cache = if crate::system::current_os() == crate::system::OperatingSystem::Windows {
        "Cache"
    } else {
        "KotavelPathTest"
    };
    assert_eq!(cache.file_name().unwrap(), expected_cache);

    for name in ["", ".", "..", "../escape", "nested/name", "nested\\name"] {
        assert_eq!(
            paths.config_directory(name).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            paths.data_directory(name).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            paths.cache_directory(name).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn runtime_paths_match_the_active_platform_layout() {
    let resolver = NativeRuntimePathResolver;
    assert_eq!(
        resolver.resolve(Path::new("")).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    let root = env::current_dir().unwrap().join("kotavel-platform-fixture");
    let resolved = match crate::system::current_os() {
        crate::system::OperatingSystem::Linux => {
            let executable = root.join("prefix/bin/kotavel");
            let paths = resolver.resolve(&executable).unwrap();
            assert_eq!(paths.application_root, root.join("prefix"));
            assert_eq!(paths.plugins, root.join("prefix/lib/kotavel/plugins"));
            assert_eq!(
                paths.translations,
                root.join("prefix/share/kotavel/translations")
            );
            assert_eq!(
                paths.certificates,
                root.join("prefix/share/kotavel/certificates")
            );
            paths
        }
        crate::system::OperatingSystem::MacOS => {
            let bundle = root.join("Kotavel.app");
            let paths = resolver
                .resolve(&bundle.join("Contents/MacOS/Kotavel"))
                .unwrap();
            assert_eq!(paths.application_root, bundle);
            assert_eq!(paths.plugins, root.join("Kotavel.app/Contents/PlugIns"));
            assert_eq!(
                paths.translations,
                root.join("Kotavel.app/Contents/Resources/translations")
            );
            assert_eq!(
                paths.certificates,
                root.join("Kotavel.app/Contents/Resources/certificates")
            );
            paths
        }
        crate::system::OperatingSystem::Windows => {
            let directory = root.join("application");
            let paths = resolver.resolve(&directory.join("Kotavel.exe")).unwrap();
            assert_eq!(paths.application_root, directory);
            assert_eq!(paths.plugins, root.join("application/plugins"));
            assert_eq!(paths.translations, root.join("application/translations"));
            assert_eq!(paths.certificates, root.join("application/certificates"));
            paths
        }
        crate::system::OperatingSystem::Unknown => return,
    };
    assert!(resolved.application_root.is_absolute());
}

#[test]
fn packaged_resources_require_a_valid_kotavel_bundle_marker() {
    let directory = tempfile::tempdir().unwrap();
    let resources = directory.path().join("Kotavel.app/Contents/Resources");
    let executable = directory.path().join("Kotavel.app/Contents/MacOS/Kotavel");
    std::fs::create_dir_all(&resources).unwrap();
    std::fs::write(resources.join("kotavel-environment"), "development\n").unwrap();
    if crate::system::current_os() == crate::system::OperatingSystem::MacOS {
        assert_eq!(
            packaged_resources_root(&executable),
            Some(resources.clone())
        );
    } else {
        assert_eq!(packaged_resources_root(&executable), None);
    }
    std::fs::write(resources.join("kotavel-environment"), "invalid\n").unwrap();
    assert_eq!(packaged_resources_root(&executable), None);
}
