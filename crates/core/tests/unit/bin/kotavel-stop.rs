use super::*;
use std::ffi::OsStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn process_id_accepts_only_positive_numeric_endpoint_names() {
    for (name, expected) in [
        ("kotavel-42.endpoint", Some(42)),
        ("kotavel-0.endpoint", None),
        ("kotavel--2.endpoint", None),
        ("kotavel-abc.endpoint", None),
        ("other-42.endpoint", None),
        ("kotavel-42.endpoint.bak", None),
    ] {
        assert_eq!(process_id(Path::new(name)), expected, "{name}");
    }
}

#[test]
fn process_identity_requires_binary_inside_build_and_exact_project_root() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let project = env::temp_dir().join(format!("kotavel-stop-{unique}"));
    let build = project.join("build/Debug");
    fs::create_dir_all(&build).unwrap();
    let binary = build.join("Kotavel");
    fs::write(&binary, []).unwrap();
    let outside = project.join("other/Kotavel");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, []).unwrap();
    let project = project.canonicalize().unwrap();

    assert!(!process_matches_identity(
        Some(&binary),
        &[OsStr::new("kotavel"), OsStr::new("--project-root=")],
        &project,
    ));
    assert!(process_matches_identity(
        Some(&binary),
        &[
            OsStr::new("kotavel"),
            OsStr::new(format!("--project-root={}", project.display()).as_str()),
        ],
        &project,
    ));
    assert!(!process_matches_identity(
        Some(&outside),
        &[
            OsStr::new("kotavel"),
            OsStr::new(format!("--project-root={}", project.display()).as_str()),
        ],
        &project,
    ));
    fs::remove_dir_all(project).unwrap();
}

fn process_matches_identity(executable: Option<&Path>, command: &[&OsStr], project: &Path) -> bool {
    let Ok(project) = project.canonicalize() else {
        return false;
    };
    let Some(executable_path) = executable else {
        return false;
    };
    let executable_path = if executable_path.is_absolute() {
        executable_path.to_owned()
    } else {
        project.join(executable_path)
    };
    let Some(executable) = executable_path.canonicalize().ok() else {
        return false;
    };
    let executable_roots = ["build", "target", "dist"]
        .into_iter()
        .filter_map(|directory| project.join(directory).canonicalize().ok())
        .collect::<Vec<_>>();
    executable_roots
        .iter()
        .any(|root| executable.starts_with(root))
        && {
            let project_arguments = command
                .iter()
                .skip(1)
                .filter_map(|argument| argument.to_str()?.strip_prefix("--project-root="));
            let mut has_project_argument = false;
            let project_argument_matches = project_arguments.into_iter().any(|root| {
                has_project_argument = true;
                Path::new(root)
                    .canonicalize()
                    .is_ok_and(|root| same_path(&root, &project))
            });
            !has_project_argument || project_argument_matches
        }
}

#[test]
fn parses_project_and_pid_options() {
    let options = parse_arguments(["--project=/tmp/kotavel".into(), "--pid".into(), "42".into()])
        .unwrap()
        .unwrap();
    assert_eq!(options.project, PathBuf::from("/tmp/kotavel"));
    assert_eq!(options.pid, Some(42));
    assert!(parse_arguments(["--pid=0".into()]).is_err());
}
