"""Exercise the real HTTP adapter; optionally record exploratory loopback latency."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import http.client
import json
from pathlib import Path
import socket
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("--measure", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="kotavel-http-flow-") as temporary:
        root = Path(temporary)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        log_path = root / "server.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(args.executable.resolve()), str(port), str(root / "sessions")],
                                       stdout=log, stderr=subprocess.STDOUT)
            connection = None
            try:
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError(log_path.read_text())
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                            break
                    except OSError:
                        time.sleep(0.02)
                else:
                    raise RuntimeError("HTTP fixture did not start")
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                requests = 0

                def send(method, path, body=None, headers=None):
                    nonlocal requests
                    connection.request(method, path, body=body, headers=headers or {})
                    response = connection.getresponse()
                    payload = response.read()
                    requests += 1
                    return response.status, payload, dict(response.getheaders())

                assert send("GET", "/api/static/999")[0] == 200
                assert send("POST", "/api/static/999")[0] == 405
                assert send("OPTIONS", "/api/static/999")[0] == 403
                assert send("HEAD", "/api/static/999")[:2] == (200, b"")
                assert send("GET", "/api/missing")[0] == 404
                assert send("GET", "/api/items/42")[0] == 200
                overlap = send("GET", "/api/items/special")
                assert overlap[0] == 405, overlap
                assert send("POST", "/api/items/special")[0] == 201
                assert send("PATCH", "/api/items/special")[0] == 405
                assert send("POST", "/api/echo", "{", {"Content-Type": "application/json"})[0] == 400
                assert send("POST", "/api/echo", "{}", {"Accept": "text/html"})[0] == 406
                status, body, headers = send("GET", "/session")
                assert (status, body) == (200, b"1")
                cookie = next(value for key, value in headers.items() if key.lower() == "set-cookie").split(";", 1)[0]
                assert send("GET", "/session", headers={"Cookie": cookie})[:2] == (200, b"2")

                native_status, native_body, native_headers = send("GET", "/drogon-session")
                assert (native_status, native_body) == (200, b"1")
                native_cookie = next(value for key, value in native_headers.items()
                                     if key.lower() == "set-cookie").split(";", 1)[0]
                assert send("GET", "/drogon-session", headers={"Cookie": native_cookie})[:2] == (200, b"2")

                def concurrent_requests(path, client_cookie):
                    def request_one(_):
                        client = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
                        try:
                            client.request("GET", path, headers={"Cookie": client_cookie})
                            response = client.getresponse()
                            return response.status, response.read()
                        finally:
                            client.close()

                    began = time.perf_counter()
                    with ThreadPoolExecutor(max_workers=64) as pool:
                        results = list(pool.map(request_one, range(64)))
                    return results, time.perf_counter() - began

                kotavel_concurrent, kotavel_elapsed = concurrent_requests("/session", cookie)
                native_concurrent, native_elapsed = concurrent_requests(
                    "/drogon-session", native_cookie)
                requests += len(kotavel_concurrent) + len(native_concurrent)
                assert all(status == 200 for status, _ in kotavel_concurrent), kotavel_concurrent
                assert all(status == 200 for status, _ in native_concurrent), native_concurrent
                kotavel_final = send("GET", "/session", headers={"Cookie": cookie})[:2]
                assert kotavel_final == (200, b"67"), kotavel_final
                native_final = send("GET", "/drogon-session", headers={"Cookie": native_cookie})[:2]
                assert native_final == (200, b"67"), native_final
                payload = json.dumps({f"field_{i}": "x" * 100 for i in range(1000)})
                status, body, _ = send("POST", "/api/echo", payload, {"Content-Type": "application/json"})
                assert status == 200 and len(json.loads(body)["data"]) == 1000
                if args.measure:
                    results = {}
                    for name, method, path, body, headers in [
                        ("get_1000_routes", "GET", "/api/static/999", None, {}),
                        ("post_100kb_json", "POST", "/api/echo", payload, {"Content-Type": "application/json"}),
                        ("session_counter", "GET", "/session", None, {"Cookie": cookie}),
                    ]:
                        samples = []
                        for i in range(110):
                            start = time.perf_counter_ns()
                            assert send(method, path, body, headers)[0] == 200
                            elapsed = (time.perf_counter_ns() - start) / 1_000_000
                            if i >= 10:
                                samples.append(elapsed)
                        samples.sort()
                        results[name] = {"p50_ms": statistics.median(samples), "p95_ms": samples[94], "p99_ms": samples[98]}
                    results["session_concurrent_64"] = {
                        "kotavel_requests_per_second": 64 / kotavel_elapsed,
                        "drogon_native_requests_per_second": 64 / native_elapsed,
                    }
                    args.measure.write_text(json.dumps(results, indent=2) + "\n")

                # Every response, including 404/405/400/406, must retain its access log.
                logs = log_path.read_text()
                assert logs.count("[HTTP] ") == requests, (requests, logs)
                for status in (200, 201, 400, 403, 404, 405, 406):
                    assert f" {status} " in logs, logs
                print(f"HTTP transport passed: {requests} responses logged")

                # SIGKILL the first process after it has acknowledged all writes,
                # then reopen the same SQLite database and verify durable recovery.
                connection.close()
                connection = None
                process.kill()
                process.wait(timeout=5)
                process = subprocess.Popen(
                    [str(args.executable.resolve()), str(port), str(root / "sessions")],
                    stdout=log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    if process.poll() is not None:
                        raise RuntimeError(log_path.read_text())
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                            break
                    except OSError:
                        time.sleep(0.02)
                else:
                    raise RuntimeError("HTTP fixture did not restart")
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
                recovered = send("GET", "/session", headers={"Cookie": cookie})
                expected_counter = 68 + (110 if args.measure else 0)
                assert recovered[:2] == (200, str(expected_counter).encode()), recovered[:2]
                recovered_cookie = next(value for key, value in recovered[2].items()
                                        if key.lower() == "set-cookie").split(";", 1)[0]
                assert recovered_cookie == cookie, "abrupt restart preserves the Kotavel session ID"
            finally:
                if connection:
                    connection.close()
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == "__main__":
    main()
