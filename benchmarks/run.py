"""Repite el microbenchmark y conserva muestras y entorno, sin comparar frameworks."""
import argparse
import csv
import hashlib
import io
import json
import platform
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("executable", type=Path)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
sources = sorted([*root.glob("src/**/*.cpp"), *root.glob("src/**/*.hpp"),
                  *root.glob("include/**/*.hpp"), root / "CMakeLists.txt",
                  *root.glob("benchmarks/*.cpp")])
digest = hashlib.sha256()
for path in sources:
    digest.update(path.relative_to(root).as_posix().encode())
    digest.update(path.read_bytes())
runs = []
for run in range(3):
    output = subprocess.run([str(args.executable.resolve())], check=True,
                            capture_output=True, text=True).stdout
    rows = list(csv.DictReader(io.StringIO(output)))
    runs.append([{key: value if key == "scenario" else float(value)
                  for key, value in row.items()} for row in rows])
result = {"kind": "exploratory_microbenchmark", "system": platform.platform(),
          "machine": platform.machine(), "source_sha256": digest.hexdigest(),
          "note": "Record build flags/compiler and hardware separately; not a production budget.",
          "runs": runs}
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(f"3 ejecuciones guardadas en {args.output}")
