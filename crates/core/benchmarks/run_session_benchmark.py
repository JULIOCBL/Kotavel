#!/usr/bin/env python3
"""Repeat and validate the Rust Kotavel session acceptance benchmark."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import platform
import statistics
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HASHED_SOURCES = (
    "src/session/cache.rs",
    "src/session/manager.rs",
    "src/session/sqlite.rs",
    "src/bin/kotavel-session-benchmark.rs",
    "benchmarks/run_session_benchmark.py",
    "src/metrics/native.rs",
    "src/metrics/mod.rs",
)
NUMERIC_FIELDS = (
    "clients", "sessions", "offered_rps", "requests", "writes_committed",
    "conflicts", "failures", "elapsed_s", "p50_ms", "p95_ms", "p99_ms",
    "write_p50_ms", "write_p95_ms", "write_p99_ms", "throughput_rps",
    "cache_hits", "cache_misses", "cache_hit_rate_pct", "cache_lock_contended",
    "cache_lock_wait_total_ms", "cache_lock_wait_mean_ms", "cache_lock_wait_max_ms",
    "resident_sessions", "resident_bytes", "writer_jobs", "writer_queue_mean_ms",
    "writer_queue_max_ms", "writer_service_total_ms", "writer_service_max_ms",
    "writer_outstanding", "writer_peak_outstanding", "write_batches",
    "batched_write_jobs", "mean_batch_size", "max_batch_size", "process_cpu_mean_pct",
    "process_cpu_peak_pct", "process_metric_samples", "process_peak_rss_bytes",
)
SUMMARY_FIELDS = (
    "throughput_rps", "p50_ms", "p95_ms", "p99_ms", "write_p95_ms",
    "write_p99_ms", "cache_hit_rate_pct", "cache_lock_contended",
    "cache_lock_wait_mean_ms", "cache_lock_wait_max_ms", "resident_bytes",
    "writer_queue_max_ms", "writer_service_max_ms", "writer_peak_outstanding",
    "process_cpu_mean_pct", "process_cpu_peak_pct", "process_peak_rss_bytes",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path, help="path to kotavel-session-benchmark")
    parser.add_argument("--clients", type=int, default=500)
    parser.add_argument("--seconds", type=int, default=10)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument(
        "--workloads", nargs="+", choices=("independent", "shared", "shared-keys"),
        default=("independent", "shared", "shared-keys"),
    )
    parser.add_argument("--batch-window-us", type=int, default=0)
    parser.add_argument("--build", default="unspecified")
    parser.add_argument("--output", type=Path, help="write the JSON report to this path")
    args = parser.parse_args()
    if min(args.clients, args.seconds, args.repeats) < 1:
        parser.error("clients, seconds and repeats must be positive")
    if args.clients > 4096 or args.seconds > 3600 or args.repeats > 100:
        parser.error("clients must be <=4096, seconds <=3600 and repeats <=100")
    if not 0 <= args.batch_window_us <= 10_000:
        parser.error("batch-window-us must be between 0 and 10000")
    args.executable = args.executable.resolve()
    if not args.executable.is_file():
        parser.error(f"benchmark executable does not exist: {args.executable}")
    return args


def source_digest() -> str:
    digest = hashlib.sha256()
    for relative in HASHED_SOURCES:
        digest.update(relative.encode("utf-8"))
        digest.update((ROOT / relative).read_bytes())
    return digest.hexdigest()


def expected_writes(clients: int, seconds: int) -> int:
    rounds = seconds * 2
    full_cycles, remainder = divmod(rounds, 5)
    return clients * full_cycles + sum(
        sum((client + round_index) % 5 == 4 for round_index in range(remainder))
        for client in range(clients)
    )


def run_profile(args: argparse.Namespace, workload: str, repeat: int) -> dict[str, object]:
    command = [
        str(args.executable), str(args.clients), str(args.seconds), workload,
        str(args.batch_window_us),
    ]
    completed = subprocess.run(
        command, check=False, capture_output=True, text=True,
        timeout=args.seconds * 2 + 120,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{workload} run {repeat} exited {completed.returncode}: "
            f"{completed.stderr.strip()}\n{completed.stdout.strip()}"
        )
    rows = list(csv.DictReader(io.StringIO(completed.stdout)))
    if len(rows) != 1:
        raise RuntimeError(f"expected one CSV row for {workload}, received {len(rows)}")
    row: dict[str, object] = {
        key: float(value) if key in NUMERIC_FIELDS else value
        for key, value in rows[0].items()
    }
    offered_requests = args.clients * args.seconds * 2
    if row["workload"] != workload or row["requests"] != offered_requests:
        raise RuntimeError(
            f"{workload} run {repeat}: expected {offered_requests} requests, "
            f"got {row['requests']}"
        )
    if row["offered_rps"] != args.clients * 2:
        raise RuntimeError(f"{workload} run {repeat}: offered rate differs from configuration")
    if row["conflicts"] != 0 or row["failures"] != 0:
        raise RuntimeError(
            f"{workload} run {repeat}: conflicts={row['conflicts']}, failures={row['failures']}"
        )
    writes = expected_writes(args.clients, args.seconds)
    if row["writes_committed"] != writes:
        raise RuntimeError(
            f"{workload} run {repeat}: expected {writes} writes, got {row['writes_committed']}"
        )
    if (
        rows[0]["cache_verified"] != "true"
        or rows[0]["durable_verified"] != "true"
        or row["writer_outstanding"] != 0
    ):
        raise RuntimeError(
            f"{workload} run {repeat}: cache/durable state or writer queue did not settle"
        )
    row["repeat"] = repeat
    return row


def summarize(runs: list[dict[str, object]]) -> dict[str, dict[str, float]]:
    summary = {
        field: {
            "median": statistics.median(float(run[field]) for run in runs),
            "min": min(float(run[field]) for run in runs),
            "max": max(float(run[field]) for run in runs),
        }
        for field in SUMMARY_FIELDS
    }
    summary["conflicts"] = {"median": 0.0, "min": 0.0, "max": 0.0}
    summary["failures"] = {"median": 0.0, "min": 0.0, "max": 0.0}
    return summary


def main() -> int:
    args = parse_args()
    report: dict[str, object] = {
        "description": "Repeated Rust Kotavel session benchmark; each run verifies durable writes.",
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "system": f"{platform.system()}-{platform.release()}-{platform.machine()}",
        "build": args.build,
        "executable": str(args.executable),
        "configuration": {
            "clients": args.clients,
            "seconds": args.seconds,
            "repeats": args.repeats,
            "offered_rps": args.clients * 2,
            "write_fraction": 0.2,
            "batch_window_us": args.batch_window_us,
            "workloads": args.workloads,
        },
        "source_sha256": source_digest(),
        "hashed_sources": list(HASHED_SOURCES),
        "runs": [],
        "summary": {},
    }
    runs_by_workload: dict[str, list[dict[str, object]]] = {}
    for workload in args.workloads:
        runs = [run_profile(args, workload, repeat) for repeat in range(1, args.repeats + 1)]
        runs_by_workload[workload] = runs
        report["runs"].extend(runs)  # type: ignore[union-attr]
    report["summary"] = {
        workload: summarize(runs) for workload, runs in runs_by_workload.items()
    }
    serialized = json.dumps(report, indent=2) + "\n"
    if args.output:
        output = args.output.resolve()
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(serialized, encoding="utf-8")
    sys.stdout.write(serialized)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.SubprocessError, RuntimeError, ValueError) as error:
        print(f"Rust session benchmark runner failed: {error}", file=sys.stderr)
        raise SystemExit(1)
