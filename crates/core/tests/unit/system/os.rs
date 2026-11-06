use super::*;

#[test]
fn current_platform_has_a_stable_name_and_key() {
    let os = current_os();
    assert!(!os.name().is_empty());
    assert!(os.key().bytes().all(|byte| byte.is_ascii_lowercase()));
    assert_eq!(os.is_windows(), os == OperatingSystem::Windows);
    assert_eq!(os.is_macos(), os == OperatingSystem::MacOS);
    assert_eq!(os.is_linux(), os == OperatingSystem::Linux);
}
