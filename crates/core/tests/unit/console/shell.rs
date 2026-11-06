use super::*;

#[test]
fn chooses_clean_interactive_shell_arguments_for_supported_shells() {
    assert_eq!(
        unix_shell_arguments(Path::new("/bin/bash")),
        ["--noprofile", "--norc", "-i"]
    );
    assert_eq!(unix_shell_arguments(Path::new("/bin/zsh")), ["-i"]);
    assert_eq!(power_shell_arguments(), ["-NoLogo", "-NoProfile"]);
}

#[cfg(unix)]
#[test]
fn shell_starts_in_project_and_exports_its_root() {
    let project = tempfile::tempdir().unwrap();
    let path = project.path().canonicalize().unwrap();
    let command = shell_command(&project.path().join(".")).unwrap();

    assert_eq!(command.get_current_dir(), Some(path.as_path()));
    assert!(
        command.get_envs().any(|(key, value)| {
            key == "KOTAVEL_PROJECT_ROOT" && value == Some(path.as_os_str())
        })
    );
}

#[test]
fn refuses_to_spawn_a_shell_without_an_interactive_terminal() {
    if !io::stdin().is_terminal() {
        assert!(matches!(
            run_interactive_shell(Path::new(".")),
            Err(InteractiveShellError::NonInteractive)
        ));
    }
}
