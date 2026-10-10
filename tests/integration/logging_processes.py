"""Comprueba el bloqueo real de una carpeta entre procesos, sin red."""
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory(prefix="kotavel-process-lock-") as directory:
    owner = subprocess.Popen([sys.argv[1], "--hold", directory], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, text=True)
    try:
        assert owner.stdout.readline().strip() == "ready", "owner did not acquire lock"
        blocked = subprocess.run([sys.argv[1], "--probe", directory], timeout=5)
        assert blocked.returncode == 42, "second process acquired the same log folder"
    finally:
        owner.communicate("release\n", timeout=5)
    assert owner.returncode == 0
    released = subprocess.run([sys.argv[1], "--probe", directory], timeout=5)
    assert released.returncode == 0, "lock was not released on shutdown"
