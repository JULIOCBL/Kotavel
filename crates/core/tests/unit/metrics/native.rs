use super::*;

#[test]
fn native_sampler_reports_nonzero_resident_memory() {
    let Some(sample) = NativeProcessMetricsProvider.read() else {
        panic!("native process metrics unavailable on this target");
    };
    assert!(sample.resident_memory_bytes > 0);
    assert!(sample.cpu_time <= Duration::from_secs(365 * 24 * 60 * 60));
}
