use super::*;

#[test]
fn parses_default_and_explicit_benchmark_profiles() {
    let defaults = Options::parse(Vec::<String>::new()).unwrap().unwrap();
    assert_eq!(defaults.clients, 500);
    assert_eq!(defaults.seconds, 5);
    assert_eq!(defaults.workload, Workload::Independent);
    assert_eq!(defaults.batch_window_us, 0);

    let shared = Options::parse(
        ["64", "2", "shared-keys", "2500"]
            .into_iter()
            .map(str::to_owned),
    )
    .unwrap()
    .unwrap();
    assert_eq!(shared.clients, 64);
    assert_eq!(shared.seconds, 2);
    assert_eq!(shared.workload, Workload::SharedKeys);
    assert_eq!(shared.batch_window_us, 2500);
}

#[test]
fn rejects_invalid_benchmark_bounds_and_workloads() {
    for arguments in [
        vec!["0"],
        vec!["4097"],
        vec!["10", "0"],
        vec!["10", "1", "unknown"],
        vec!["10", "1", "shared", "10001"],
        vec!["10", "1", "shared", "-1"],
    ] {
        assert!(Options::parse(arguments.into_iter().map(str::to_owned)).is_err());
    }
    assert!(
        Options::parse(["--help"].into_iter().map(str::to_owned))
            .unwrap()
            .is_none()
    );
}
