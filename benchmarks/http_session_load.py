#!/usr/bin/env python3
"""Drive real concurrent HTTP session requests through the Drogon adapter."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import http.client
import json
import math
from pathlib import Path
import platform
import socket
import statistics
import subprocess
import tempfile
import threading
import time


def percentile(samples, fraction):
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(fraction * len(ordered)) - 1)]


def reserve_port():
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        return reservation.getsockname()[1]


def start_server(executable, port, storage, log):
    process = subprocess.Popen(
        [str(executable), str(port), str(storage)], stdout=log, stderr=subprocess.STDOUT
    )
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("Drogon session fixture exited during startup")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                return process
        except OSError:
            time.sleep(0.02)
    process.kill()
    process.wait()
    raise TimeoutError("Drogon session fixture did not start")


def start_servers(executable, ports, storage, logs):
    processes = []
    try:
        for port, log in zip(ports, logs):
            processes.append(start_server(executable, port, storage, log))
        return processes
    except Exception:
        stop_servers(processes, force=False)
        raise


def stop_servers(processes, force):
    for process in processes:
        if process.poll() is None:
            process.kill() if force else process.terminate()
    for process in processes:
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def send(port, path, cookie=None):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
    try:
        return send_on(connection, path, cookie)
    finally:
        connection.close()


def send_on(connection, path, cookie):
    headers = {"Cookie": cookie} if cookie else {}
    connection.request("GET", path, headers=headers)
    response = connection.getresponse()
    return response.status, response.read(), response.getheaders()


def extract_cookie(headers):
    for key, value in headers:
        if key.lower() == "set-cookie":
            return value.split(";", 1)[0]
    raise RuntimeError("session response did not set a cookie")


def create_session(port):
    status, body, headers = send(port, "/session/read")
    if status != 200 or body != b"0":
        raise AssertionError(("session creation", status, body))
    return extract_cookie(headers)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path, help="Drogon-enabled transport fixture")
    parser.add_argument("--clients", type=int, default=500)
    parser.add_argument("--seconds", type=int, default=10)
    parser.add_argument("--instances", type=int, default=1,
                        help="independent Drogon processes sharing the same SQLite file")
    parser.add_argument("--workload", choices=("independent", "shared"), default="independent")
    parser.add_argument("--build", default="unspecified")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.clients < 1 or args.seconds < 1 or args.instances < 1:
        parser.error("clients, seconds and instances must be positive")
    executable = args.executable.resolve()
    if not executable.is_file():
        parser.error(f"fixture does not exist: {executable}")

    with tempfile.TemporaryDirectory(prefix="kotavel-http-session-load-") as temporary:
        root = Path(temporary)
        storage = root / "storage"
        ports = [reserve_port() for _ in range(args.instances)]
        logs = [(root / f"server-{index}.log").open("w") for index in range(args.instances)]
        processes = []
        try:
            processes = start_servers(executable, ports, storage, logs)
            session_count = args.clients if args.workload == "independent" else 1
            cookies = [create_session(ports[0]) for _ in range(session_count)]
            ready = threading.Barrier(args.clients + 1)
            start_at = time.monotonic() + 1.0
            latencies = [[] for _ in range(args.clients)]
            failures = [[] for _ in range(args.clients)]
            request_count = [0] * args.clients
            write_count = [0] * args.clients
            periods = args.seconds * 2

            def run_client(client_id):
                cookie = cookies[0] if args.workload == "shared" else cookies[client_id]
                port = ports[client_id % len(ports)]
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
                try:
                    ready.wait()
                    for round_id in range(periods):
                        due = start_at + round_id * 0.5 + 0.5 * client_id / args.clients
                        delay = due - time.monotonic()
                        if delay > 0:
                            time.sleep(delay)
                        write = round_id % 5 == 4
                        path = "/session/write" if write else "/session/read"
                        began = time.perf_counter_ns()
                        try:
                            status, _body, _headers = send_on(connection, path, cookie)
                            elapsed = (time.perf_counter_ns() - began) / 1_000_000
                            latencies[client_id].append(elapsed)
                            request_count[client_id] += 1
                            if status != 200:
                                failures[client_id].append(f"HTTP {status}")
                            elif write:
                                write_count[client_id] += 1
                        except Exception as error:  # retain all worker errors for the report
                            failures[client_id].append(repr(error))
                            break
                finally:
                    connection.close()

            with ThreadPoolExecutor(max_workers=args.clients) as pool:
                futures = [pool.submit(run_client, client_id)
                           for client_id in range(args.clients)]
                ready.wait()
                for future in futures:
                    future.result()
            elapsed_seconds = time.monotonic() - start_at

            expected_requests = args.clients * periods
            writes_per_client = sum(round_id % 5 == 4 for round_id in range(periods))
            expected_writes = args.clients * writes_per_client
            requests = sum(request_count)
            writes = sum(write_count)
            errors = [error for client_errors in failures for error in client_errors]
            if requests != expected_requests or writes != expected_writes or errors:
                raise AssertionError({
                    "expected_requests": expected_requests,
                    "requests": requests,
                    "expected_writes": expected_writes,
                    "writes": writes,
                    "errors": errors[:10],
                })

            expected_final = expected_writes
            if args.workload == "independent":
                recovered = [send(ports[index % len(ports)], "/session/read", cookie)
                             for index, cookie in enumerate(cookies)]
                bad = [(status, body) for status, body, _headers in recovered
                       if status != 200 or body != str(writes_per_client).encode()]
                if bad:
                    raise AssertionError(("independent durable counters", bad[:10]))
            else:
                verify_port = ports[-1]
                status, body, _headers = send(verify_port, "/session/read", cookies[0])
                if status != 200 or int(body) != expected_final:
                    raise AssertionError(("shared durable counter", status, body, expected_final))

            stop_servers(processes, force=True)
            processes = start_servers(executable, ports, storage, logs)
            if args.workload == "independent":
                recovered = [send(ports[index % len(ports)], "/session/read", cookie)
                             for index, cookie in enumerate(cookies)]
                bad = [(status, body) for status, body, _headers in recovered
                       if status != 200 or body != str(writes_per_client).encode()]
                if bad:
                    raise AssertionError(("independent restart recovery", bad[:10]))
            else:
                verify_port = ports[-1]
                status, body, _headers = send(verify_port, "/session/read", cookies[0])
                if status != 200 or int(body) != expected_final:
                    raise AssertionError(("shared restart recovery", status, body, expected_final))

            samples = [sample for client_samples in latencies for sample in client_samples]
            result = {
                "timestamp_utc": datetime.now(timezone.utc).isoformat(),
                "system": platform.platform(),
                "build": args.build,
                "source_sha256": source_hash(),
                "workload": args.workload,
                "clients": args.clients,
                "instances": args.instances,
                "seconds": args.seconds,
                "offered_rps": args.clients * 2,
                "requests": requests,
                "writes": writes,
                "write_fraction": writes / requests,
                "throughput_rps": requests / elapsed_seconds,
                "p50_ms": statistics.median(samples),
                "p95_ms": percentile(samples, 0.95),
                "p99_ms": percentile(samples, 0.99),
                "failures": len(errors),
                "durable_writes_recovered": writes,
            }
            print(json.dumps(result, indent=2))
            if args.output:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(json.dumps(result, indent=2) + "\n")
                print(f"wrote {args.output}")
        finally:
            stop_servers(processes, force=False)
            for log in logs:
                log.close()


def source_hash():
    root = Path(__file__).resolve().parents[1]
    sources = (
        "benchmarks/http_session_load.py",
        "tests/integration/http_transport_fixture.cpp",
        "src/http/session_middleware.cpp",
        "src/session/session.cpp",
        "src/session/cached_session_store.cpp",
        "src/session/sqlite_session_store.cpp",
        "include/kotavel/contracts/session_store.hpp",
        "include/kotavel/session/session.hpp",
    )
    digest = hashlib.sha256()
    for relative in sources:
        digest.update(relative.encode())
        digest.update((root / relative).read_bytes())
    return digest.hexdigest()


if __name__ == "__main__":
    main()
