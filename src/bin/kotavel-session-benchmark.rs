use kotavel::metrics::{MetricsSample, NativeProcessMetricsProvider, ProcessMetricsCollector};
use kotavel::session::{
    CacheOptions, CachedSessionStore, SQLiteSessionStore, SessionConfig, SessionManager,
    SessionStore, StoreOptions,
};
use serde_json::Value;
use std::{
    env,
    error::Error,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Workload {
    Independent,
    Shared,
    SharedKeys,
}

impl Workload {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "independent" => Ok(Self::Independent),
            "shared" => Ok(Self::Shared),
            "shared-keys" => Ok(Self::SharedKeys),
            _ => Err("workload must be independent, shared, or shared-keys".into()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Shared => "shared",
            Self::SharedKeys => "shared-keys",
        }
    }
}

struct Options {
    clients: usize,
    seconds: usize,
    workload: Workload,
    batch_window_us: u64,
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let arguments = arguments.into_iter();
        let mut values = Vec::new();
        for argument in arguments {
            if argument == "-h" || argument == "--help" {
                return Ok(None);
            }
            if argument.starts_with('-') {
                return Err(format!("unknown option: {argument}"));
            }
            values.push(argument);
        }
        if values.len() > 4 {
            return Err(
                "usage: kotavel-session-benchmark [clients] [seconds] [independent|shared|shared-keys] [batch-window-us]".into(),
            );
        }
        let parse_positive = |index: usize, default: usize, label: &str| {
            values.get(index).map_or(Ok(default), |value| {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|parsed| *parsed > 0)
                    .ok_or_else(|| format!("{label} must be a positive integer"))
            })
        };
        let clients = parse_positive(0, 500, "clients")?;
        let seconds = parse_positive(1, 5, "seconds")?;
        let workload = values
            .get(2)
            .map_or(Ok(Workload::Independent), |value| Workload::parse(value))?;
        let batch_window_us = values.get(3).map_or(Ok(0), |value| {
            value
                .parse::<u64>()
                .map_err(|_| "batch window must be an integer".to_owned())
        })?;
        if clients > 4096 || seconds > 3600 || batch_window_us > 10_000 {
            return Err(
                "clients must be <=4096, seconds <=3600 and batch window <=10000 microseconds"
                    .into(),
            );
        }
        Ok(Some(Self {
            clients,
            seconds,
            workload,
            batch_window_us,
        }))
    }
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            eprintln!("session benchmark failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let Some(options) = Options::parse(env::args().skip(1))? else {
        print_help();
        return Ok(());
    };
    let offered_requests = options.clients * options.seconds * 2;
    let session_count = match options.workload {
        Workload::Independent => options.clients,
        Workload::Shared | Workload::SharedKeys => 1,
    };
    let directory = tempfile::tempdir()?;
    let backing = Arc::new(SQLiteSessionStore::open(
        directory.path().join("sessions.sqlite3"),
        StoreOptions {
            write_batch_window: Duration::from_micros(options.batch_window_us),
            ..StoreOptions::default()
        },
    )?);
    let cache = Arc::new(CachedSessionStore::new(
        Arc::clone(&backing) as Arc<dyn SessionStore>,
        CacheOptions {
            max_sessions: options.clients.saturating_add(64).max(2048),
            max_bytes: 32 * 1024 * 1024,
            ttl: Duration::from_secs(options.seconds as u64 + 60),
            shards: 64,
        },
    )?);
    let manager = Arc::new(SessionManager::new(
        Arc::clone(&cache) as Arc<dyn SessionStore>,
        SessionConfig::default(),
    )?);

    let mut session_ids = Vec::with_capacity(session_count);
    for _ in 0..session_count {
        let mut session = manager.begin(None)?;
        session.put("writes", Value::from(0))?;
        if !session.commit()? {
            return Err("could not create benchmark session".into());
        }
        session_ids.push(session.id().to_owned());
    }

    let mut sampler = ProcessSampler::start()?;

    let rounds = options.seconds * 2;
    let start_gate = Arc::new(Barrier::new(options.clients + 1));
    let start = Instant::now();
    let outcomes = thread::scope(|scope| {
        let workers = (0..options.clients)
            .map(|client| {
                let manager = Arc::clone(&manager);
                let session_ids = &session_ids;
                let start_gate = Arc::clone(&start_gate);
                let workload = options.workload;
                scope.spawn(move || -> Result<_, String> {
                    let mut latencies = Vec::with_capacity(rounds);
                    let mut write_latencies = Vec::with_capacity(rounds / 5 + 1);
                    let mut writes_committed = 0usize;
                    let mut conflicts = 0usize;
                    start_gate.wait();
                    let phase = Duration::from_millis((500 * client / options.clients) as u64);
                    for round in 0..rounds {
                        let deadline = start + phase + Duration::from_millis((500 * round) as u64);
                        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
                            thread::sleep(delay);
                        }
                        let write_request = (client + round) % 5 == 4;
                        let session_index = if workload == Workload::Independent {
                            client
                        } else {
                            0
                        };
                        let key = if workload == Workload::SharedKeys {
                            format!("writes.{client}")
                        } else {
                            "writes".to_owned()
                        };
                        let request_started = Instant::now();
                        let result = (|| -> Result<bool, kotavel::session::StoreError> {
                            let mut session = manager.begin(Some(&session_ids[session_index]))?;
                            if write_request {
                                session.increment(&key, 1)?;
                            } else {
                                let _ = session.get(&key);
                            }
                            session.commit()
                        })()
                        .map_err(|error| error.to_string())?;
                        let elapsed = request_started.elapsed();
                        latencies.push(elapsed);
                        if write_request {
                            write_latencies.push(elapsed);
                        }
                        if !result {
                            conflicts += 1;
                        } else if write_request {
                            writes_committed += 1;
                        }
                    }
                    Ok((latencies, write_latencies, writes_committed, conflicts))
                })
            })
            .collect::<Vec<_>>();
        start_gate.wait();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("benchmark worker panicked"))
            .collect::<Result<Vec<_>, _>>()
    })
    .map_err(std::io::Error::other)?;
    let elapsed = start.elapsed().as_secs_f64();
    sampler.stop()?;

    let mut latencies = Vec::with_capacity(offered_requests);
    let mut write_latencies = Vec::with_capacity(offered_requests / 5 + 1);
    let mut writes_committed = 0usize;
    let mut conflicts = 0usize;
    let mut writes_per_client = Vec::with_capacity(options.clients);
    for (requests, writes, committed, conflicted) in outcomes {
        latencies.extend(requests);
        write_latencies.extend(writes);
        writes_committed += committed;
        conflicts += conflicted;
        writes_per_client.push(committed);
    }
    let failures = 0usize;
    let completed_requests = latencies.len();
    latencies.sort_unstable();
    write_latencies.sort_unstable();
    let cache_verified =
        validate_cached_results(&manager, &session_ids, options.workload, &writes_per_client)?;
    let durable_verified =
        validate_persisted_results(&backing, &session_ids, options.workload, &writes_per_client)?;

    let cache_stats = cache.statistics();
    let writer_stats = backing.statistics();
    let process_stats = summarize_process_samples(&sampler.samples());
    let cache_requests = cache_stats.hits + cache_stats.misses;
    let hit_rate = if cache_requests == 0 {
        0.0
    } else {
        100.0 * cache_stats.hits as f64 / cache_requests as f64
    };
    println!(
        "backend,workload,clients,sessions,offered_rps,requests,writes_committed,conflicts,failures,elapsed_s,p50_ms,p95_ms,p99_ms,write_p50_ms,write_p95_ms,write_p99_ms,throughput_rps,cache_hits,cache_misses,cache_hit_rate_pct,cache_lock_contended,cache_lock_wait_total_ms,cache_lock_wait_mean_ms,cache_lock_wait_max_ms,resident_sessions,resident_bytes,writer_jobs,writer_queue_mean_ms,writer_queue_max_ms,writer_service_total_ms,writer_service_max_ms,writer_outstanding,writer_peak_outstanding,write_batches,batched_write_jobs,mean_batch_size,max_batch_size,process_cpu_mean_pct,process_cpu_peak_pct,process_metric_samples,process_peak_rss_bytes,cache_verified,durable_verified"
    );
    println!(
        "sqlite,{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.2},{},{},{:.2},{},{:.6},{:.6},{:.6},{},{},{},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{:.3},{},{:.2},{:.2},{},{},{},{}",
        options.workload.label(),
        options.clients,
        session_count,
        options.clients * 2,
        completed_requests,
        writes_committed,
        conflicts,
        failures,
        elapsed,
        percentile_ms(&latencies, 0.50),
        percentile_ms(&latencies, 0.95),
        percentile_ms(&latencies, 0.99),
        percentile_ms(&write_latencies, 0.50),
        percentile_ms(&write_latencies, 0.95),
        percentile_ms(&write_latencies, 0.99),
        completed_requests as f64 / elapsed,
        cache_stats.hits,
        cache_stats.misses,
        hit_rate,
        cache_stats.lock_contended,
        cache_stats.lock_wait.as_secs_f64() * 1000.0,
        average_ms(cache_stats.lock_wait, cache_stats.lock_contended),
        cache_stats.max_lock_wait.as_secs_f64() * 1000.0,
        cache_stats.resident_sessions,
        cache_stats.resident_bytes,
        writer_stats.write_jobs,
        average_ms(writer_stats.queue_wait, writer_stats.write_jobs),
        writer_stats.max_queue_wait.as_secs_f64() * 1000.0,
        writer_stats.service_time.as_secs_f64() * 1000.0,
        writer_stats.max_service_time.as_secs_f64() * 1000.0,
        writer_stats.outstanding_jobs,
        writer_stats.peak_outstanding_jobs,
        writer_stats.write_batches,
        writer_stats.batched_write_jobs,
        average(writer_stats.batched_write_jobs, writer_stats.write_batches),
        writer_stats.max_batch_size,
        process_stats.cpu_mean_pct,
        process_stats.cpu_peak_pct,
        process_stats.samples,
        process_stats.peak_rss_bytes,
        cache_verified,
        durable_verified,
    );
    if completed_requests != offered_requests
        || failures != 0
        || conflicts != 0
        || !durable_verified
        || !cache_verified
        || writer_stats.outstanding_jobs != 0
    {
        return Err(format!(
            "load validation failed: expected {offered_requests} requests, completed {completed_requests}, conflicts={conflicts}, failures={failures}, cache_verified={cache_verified}, durable_verified={durable_verified}, writer_outstanding={}" 
            , writer_stats.outstanding_jobs
        )
        .into());
    }
    drop(manager);
    drop(cache);
    drop(backing);
    Ok(())
}

fn validate_cached_results(
    manager: &SessionManager,
    session_ids: &[String],
    workload: Workload,
    writes_per_client: &[usize],
) -> Result<bool, Box<dyn Error>> {
    let expected_writes = writes_per_client.iter().sum::<usize>() as u64;
    match workload {
        Workload::Independent => {
            for (client, id) in session_ids.iter().enumerate() {
                let session = manager.begin(Some(id))?;
                let actual = session.get("writes").and_then(Value::as_u64);
                let expected = writes_per_client[client] as u64;
                if actual != Some(expected) {
                    eprintln!(
                        "durability check failed for client {client}: expected {expected} writes, got {actual:?}"
                    );
                    return Ok(false);
                }
            }
        }
        Workload::Shared => {
            let session = manager.begin(session_ids.first().map(String::as_str))?;
            let actual = session.get("writes").and_then(Value::as_u64);
            if actual != Some(expected_writes) {
                eprintln!(
                    "durability check failed for shared session: expected {expected_writes} writes, got {actual:?}"
                );
                return Ok(false);
            }
        }
        Workload::SharedKeys => {
            let session = manager.begin(session_ids.first().map(String::as_str))?;
            for (client, expected) in writes_per_client.iter().enumerate() {
                let actual = session
                    .get(&format!("writes.{client}"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if actual != *expected as u64 {
                    eprintln!(
                        "durability check failed for shared-keys client {client}: expected {expected} writes, got {actual}"
                    );
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn validate_persisted_results(
    store: &SQLiteSessionStore,
    session_ids: &[String],
    workload: Workload,
    writes_per_client: &[usize],
) -> Result<bool, Box<dyn Error>> {
    let expected_writes = writes_per_client.iter().sum::<usize>() as u64;
    match workload {
        Workload::Independent => {
            for (client, id) in session_ids.iter().enumerate() {
                let record = store.load(id)?;
                let actual = record
                    .as_ref()
                    .and_then(|record| record.values.get("writes"))
                    .and_then(Value::as_u64);
                let expected = writes_per_client[client] as u64;
                if actual != Some(expected) {
                    eprintln!(
                        "durable write check failed for client {client}: expected {expected}, got {actual:?}"
                    );
                    return Ok(false);
                }
            }
        }
        Workload::Shared => {
            let record = store.load(session_ids.first().expect("one shared session"))?;
            let actual = record
                .as_ref()
                .and_then(|record| record.values.get("writes"))
                .and_then(Value::as_u64);
            if actual != Some(expected_writes) {
                eprintln!(
                    "durable write check failed for shared session: expected {expected_writes}, got {actual:?}"
                );
                return Ok(false);
            }
        }
        Workload::SharedKeys => {
            let record = store.load(session_ids.first().expect("one shared session"))?;
            for (client, expected) in writes_per_client.iter().enumerate() {
                let actual = record
                    .as_ref()
                    .and_then(|record| record.values.get(&format!("writes.{client}")))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if actual != *expected as u64 {
                    eprintln!(
                        "durable write check failed for shared-keys client {client}: expected {expected}, got {actual}"
                    );
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

struct ProcessSampler {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<MetricsSample>>>,
    worker: Option<thread::JoinHandle<()>>,
    worker_thread: thread::Thread,
}

impl ProcessSampler {
    fn start() -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let samples = Arc::new(Mutex::new(Vec::new()));
        let worker_stop = Arc::clone(&stop);
        let worker_samples = Arc::clone(&samples);
        let worker = thread::Builder::new()
            .name("kotavel-session-benchmark-metrics".into())
            .spawn(move || {
                let mut collector =
                    ProcessMetricsCollector::new(Arc::new(NativeProcessMetricsProvider));
                if let Some(sample) = collector.sample() {
                    worker_samples
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(sample);
                }
                while !worker_stop.load(Ordering::Acquire) {
                    thread::park_timeout(Duration::from_millis(100));
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    if let Some(sample) = collector.sample() {
                        worker_samples
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push(sample);
                    }
                }
            })?;
        let worker_thread = worker.thread().clone();
        Ok(Self {
            stop,
            samples,
            worker: Some(worker),
            worker_thread,
        })
    }

    fn stop(&mut self) -> Result<(), &'static str> {
        self.stop.store(true, Ordering::Release);
        self.worker_thread.unpark();
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            return Err("process metrics sampler panicked");
        }
        Ok(())
    }

    fn samples(&self) -> std::sync::MutexGuard<'_, Vec<MetricsSample>> {
        self.samples
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for ProcessSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker_thread.unpark();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct ProcessSummary {
    cpu_mean_pct: f64,
    cpu_peak_pct: f64,
    samples: usize,
    peak_rss_bytes: u64,
}

fn summarize_process_samples(samples: &[MetricsSample]) -> ProcessSummary {
    let cpu_samples = samples
        .iter()
        .filter_map(|sample| sample.process_cpu_percent)
        .collect::<Vec<_>>();
    let cpu_total = cpu_samples.iter().sum::<f64>();
    ProcessSummary {
        cpu_mean_pct: if cpu_samples.is_empty() {
            0.0
        } else {
            cpu_total / cpu_samples.len() as f64
        },
        cpu_peak_pct: cpu_samples.into_iter().fold(0.0, f64::max),
        samples: samples.len(),
        peak_rss_bytes: samples
            .iter()
            .map(|sample| sample.resident_memory_bytes)
            .max()
            .unwrap_or_default(),
    }
}

fn percentile_ms(values: &[Duration], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = (percentile * (values.len() - 1) as f64) as usize;
    values[index].as_secs_f64() * 1000.0
}

fn average_ms(total: Duration, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total.as_secs_f64() * 1000.0 / count as f64
    }
}

fn average(total: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total as f64 / count as f64
    }
}

fn print_help() {
    println!(
        "Kotavel session benchmark\n\nUsage: kotavel-session-benchmark [clients] [seconds] [independent|shared|shared-keys] [batch-window-us]\n\nDefaults: 500 clients, 5 seconds, independent sessions, no SQLite write batching. Each client offers two requests per second with 20% writes. Output is CSV and reports in-process session-manager latency, throughput, cache use, conflicts and durable counter verification."
    );
}

#[cfg(test)]
mod tests {
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
}
