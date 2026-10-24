#!/usr/bin/env python3
"""Compare legacy file and current SQLite session HTTP fixtures on loopback."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.client
import json
import math
from pathlib import Path
import socket
import statistics
import subprocess
import tempfile
import time


def percentile(samples, fraction):
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(fraction * len(ordered)) - 1)]


def session_cookie(headers):
    for key, value in headers:
        if key.lower() == "set-cookie":
            return value.split(";", 1)[0]
    raise RuntimeError("session response did not set a cookie")


def start_server(executable, port, storage, log):
    process = subprocess.Popen(
        [str(executable), str(port), str(storage)], stdout=log, stderr=subprocess.STDOUT
    )
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("session fixture exited during startup")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return process
        except OSError:
            time.sleep(0.02)
    process.kill()
    process.wait()
    raise TimeoutError("session fixture did not start")


def request(port, cookie):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
    try:
        connection.request("GET", "/session", headers={"Cookie": cookie})
        response = connection.getresponse()
        body = response.read()
        return response.status, body, response.getheaders()
    finally:
        connection.close()


def run_once(executable, backend, clients):
    with tempfile.TemporaryDirectory(prefix=f"kotavel-{backend}-http-") as temporary:
        root = Path(temporary)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        log = (root / "server.log").open("w")
        process = None
        try:
            process = start_server(executable, port, root / "sessions", log)
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
            connection.request("GET", "/session")
            response = connection.getresponse()
            initial = response.read()
            cookie = session_cookie(response.getheaders())
            connection.close()
            if response.status != 200 or initial != b"1":
                raise AssertionError((backend, "initial request", response.status, initial))

            def send(_):
                began = time.perf_counter_ns()
                status, body, _headers = request(port, cookie)
                return status, body, (time.perf_counter_ns() - began) / 1_000_000

            began = time.perf_counter()
            with ThreadPoolExecutor(max_workers=clients) as pool:
                results = list(pool.map(send, range(clients)))
            elapsed = time.perf_counter() - began
            if any(status != 200 for status, _, _ in results):
                raise AssertionError((backend, "concurrent responses", results))
            values = {int(body) for _, body, _ in results}

            final_status, final_body, _ = request(port, cookie)
            if final_status != 200 or int(final_body) != clients + 2:
                raise AssertionError((backend, "final counter", final_status, final_body))
            process.kill()
            process.wait(timeout=5)
            process = start_server(executable, port, root / "sessions", log)
            recovered_status, recovered_body, recovered_headers = request(port, cookie)
            if recovered_status != 200 or int(recovered_body) != clients + 3:
                raise AssertionError((backend, "restart recovery", recovered_status, recovered_body))
            if session_cookie(recovered_headers) != cookie:
                raise AssertionError((backend, "session ID changed after restart"))

            latencies = [latency for _, _, latency in results]
            return {
                "requests": clients,
                "elapsed_s": elapsed,
                "throughput_rps": clients / elapsed,
                "latency_p50_ms": statistics.median(latencies),
                "latency_p95_ms": percentile(latencies, 0.95),
                "latency_p99_ms": percentile(latencies, 0.99),
                "unique_response_counter_values": len(values),
                "conflicts": 0,
                "failures": 0,
                "durable_counter_recovered": clients + 3,
            }
        finally:
            if process is not None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--file-executable", required=True, type=Path)
    parser.add_argument("--sqlite-executable", required=True, type=Path)
    parser.add_argument("--file-commit", required=True)
    parser.add_argument("--file-fixture-source", required=True, type=Path)
    parser.add_argument("--clients", type=int, default=64)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.clients < 1 or args.repeats < 1:
        parser.error("clients and repeats must be positive")

    runs = {"file": [], "sqlite": []}
    for repeat in range(args.repeats):
        order = [("file", args.file_executable), ("sqlite", args.sqlite_executable)]
        if repeat % 2:
            order.reverse()
        for backend, executable in order:
            runs[backend].append(run_once(executable.resolve(), backend, args.clients))

    sources = [
        "benchmarks/session_transport_compare.py",
        "src/http/session_middleware.cpp",
        "src/session/session.cpp",
        "src/session/cached_session_store.cpp",
        "src/session/sqlite_session_store.cpp",
        "tests/integration/http_transport_fixture.cpp",
        "include/kotavel/contracts/session_store.hpp",
        "include/kotavel/session/session.hpp",
    ]
    digest = hashlib.sha256()
    for relative in sources:
        digest.update(relative.encode())
        digest.update(Path(relative).read_bytes())
    legacy_fixture_sha256 = hashlib.sha256(args.file_fixture_source.read_bytes()).hexdigest()

    summary = {}
    for backend, values in runs.items():
        summary[backend] = {
            "median_rps": statistics.median(run["throughput_rps"] for run in values),
            "median_p95_ms": statistics.median(run["latency_p95_ms"] for run in values),
        }
    result = {
        "description": "Same-listener-style loopback burst against legacy FileSessionStore and current SQLite WAL middleware.",
        "file_baseline_commit": args.file_commit,
        "file_fixture_sha256": legacy_fixture_sha256,
        "file_fixture_threads": 16,
        "sqlite_fixture_threads": 16,
        "build": "Debug, AppleClang 15, Drogon supplied by Conan",
        "storage_difference": "FileSessionStore uses per-session file replacement and holds its session lock through the handler; SQLite uses WAL with synchronous=FULL and a bounded writer queue. Durability guarantees differ.",
        "clients": args.clients,
        "repeats": args.repeats,
        "method": "Each run starts a fresh server, creates one session, sends one incrementing request per concurrent client, checks the final counter and restart recovery, and kills the process. Individual response counters are snapshot-local, so only the final durable sum is an invariant. Backend order alternates to reduce order bias.",
        "source_sha256": digest.hexdigest(),
        "hashed_sources": sources,
        "summary": summary,
        "runs": runs,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(summary, indent=2))
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
