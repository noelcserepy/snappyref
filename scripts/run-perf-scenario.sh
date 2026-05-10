#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
scenario=${1:-import-1}
timestamp=$(date +"%Y%m%d-%H%M%S")
run_dir=${SNAPPYREF_PERF_RUN_DIR:-"$repo_root/perf-assets/runs/$timestamp-$scenario"}
asset_dir=${SNAPPYREF_PERF_ASSET_DIR:-"$repo_root/perf-assets/images"}
library_root="$run_dir/library"
binary=${SNAPPYREF_PERF_BINARY:-"$repo_root/target/release/snappyref"}

mkdir -p "$run_dir"
rm -rf "$library_root"

cargo build --release

python3 - "$binary" "$scenario" "$asset_dir" "$run_dir" "$library_root" <<'PY'
import csv
import os
import pathlib
import subprocess
import sys
import time

binary, scenario, asset_dir, run_dir, library_root = sys.argv[1:]
run_dir = pathlib.Path(run_dir)
samples_path = run_dir / "samples.csv"
env = os.environ.copy()
env["SNAPPYREF_LIBRARY_ROOT"] = library_root

cmd = [
    binary,
    f"--perf-scenario={scenario}",
    f"--perf-corpus={asset_dir}",
    f"--perf-run-dir={run_dir}",
    "--perf-log",
]
started = time.monotonic()
process = subprocess.Popen(cmd, env=env)

with samples_path.open("w", newline="") as handle:
    writer = csv.DictWriter(
        handle,
        fieldnames=["elapsed_ms", "pid", "cpu_percent", "rss_bytes"],
    )
    writer.writeheader()
    while process.poll() is None:
        elapsed_ms = (time.monotonic() - started) * 1000.0
        probe = subprocess.run(
            ["ps", "-p", str(process.pid), "-o", "pid=,%cpu=,rss="],
            capture_output=True,
            text=True,
        )
        line = probe.stdout.strip()
        if line:
            parts = line.split()
            if len(parts) >= 3:
                writer.writerow(
                    {
                        "elapsed_ms": f"{elapsed_ms:.2f}",
                        "pid": parts[0],
                        "cpu_percent": parts[1],
                        "rss_bytes": str(int(parts[2]) * 1024),
                    }
                )
                handle.flush()
        time.sleep(0.2)

returncode = process.wait()
summary_path = run_dir / "summary.json"
if summary_path.exists():
    import json

    summary = json.loads(summary_path.read_text())
    rows = list(csv.DictReader(samples_path.open()))
    idle_start = float(summary.get("time_to_idle_ms", 0.0))
    idle_rows = [row for row in rows if float(row["elapsed_ms"]) >= idle_start]
    steady_idle_rows = [row for row in rows if float(row["elapsed_ms"]) >= idle_start + 1000.0]

    def cpu_stats(selected):
        values = [float(row["cpu_percent"]) for row in selected]
        if not values:
            return {"samples": 0, "avg_cpu_percent": None, "max_cpu_percent": None}
        return {
            "samples": len(values),
            "avg_cpu_percent": sum(values) / len(values),
            "max_cpu_percent": max(values),
        }

    sample_summary = {
        "scenario": scenario,
        "time_to_idle_ms": idle_start,
        "all_samples": cpu_stats(rows),
        "idle_samples": cpu_stats(idle_rows),
        "steady_idle_samples_after_1s": cpu_stats(steady_idle_rows),
        "max_rss_bytes": max((int(row["rss_bytes"]) for row in rows), default=None),
        "final_rss_bytes": int(rows[-1]["rss_bytes"]) if rows else None,
    }
    (run_dir / "samples-summary.json").write_text(
        json.dumps(sample_summary, indent=2) + "\n"
    )
print(f"run_dir={run_dir}")
print(f"returncode={returncode}")
sys.exit(returncode)
PY
