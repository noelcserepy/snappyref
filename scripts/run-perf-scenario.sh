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
env["SNAPPYREF_PERF_IDLE_HOLD_SECS"] = os.environ.get("SNAPPYREF_PERF_IDLE_HOLD_SECS", "20")
env["SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS"] = os.environ.get(
    "SNAPPYREF_PERF_MODEL_UNLOAD_AFTER_IDLE_MS", "0"
)

cmd = [
    binary,
    f"--perf-scenario={scenario}",
    f"--perf-corpus={asset_dir}",
    f"--perf-run-dir={run_dir}",
    "--perf-log",
]


def parse_cpu_time(value):
    days = 0
    if "-" in value:
        day_text, value = value.split("-", 1)
        days = int(day_text)
    parts = value.split(":")
    if len(parts) == 3:
        hours, minutes, seconds = parts
    elif len(parts) == 2:
        hours = 0
        minutes, seconds = parts
    else:
        return float(value)
    return days * 86400 + int(hours) * 3600 + int(minutes) * 60 + float(seconds)


def select_window(rows, start_ms, end_ms):
    return [
        row
        for row in rows
        if start_ms <= float(row["elapsed_ms"]) < end_ms
    ]


started = time.monotonic()
process = subprocess.Popen(cmd, env=env)

with samples_path.open("w", newline="") as handle:
    writer = csv.DictWriter(
        handle,
        fieldnames=[
            "elapsed_ms",
            "pid",
            "cpu_percent",
            "cpu_time_seconds",
            "rss_bytes",
        ],
    )
    writer.writeheader()
    while process.poll() is None:
        elapsed_ms = (time.monotonic() - started) * 1000.0
        probe = subprocess.run(
            ["ps", "-p", str(process.pid), "-o", "pid=,%cpu=,time=,rss="],
            capture_output=True,
            text=True,
        )
        line = probe.stdout.strip()
        if line:
            parts = line.split()
            if len(parts) >= 4:
                writer.writerow(
                    {
                        "elapsed_ms": f"{elapsed_ms:.2f}",
                        "pid": parts[0],
                        "cpu_percent": parts[1],
                        "cpu_time_seconds": str(parse_cpu_time(parts[2])),
                        "rss_bytes": str(int(parts[3]) * 1024),
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
    idle_tail_rows = select_window(rows, idle_start, idle_start + 3000.0)
    quiescent_rows = select_window(rows, idle_start + 5000.0, idle_start + 15000.0)
    shutdown_excluded_rows = [
        row for row in rows
        if float(row["elapsed_ms"]) >= idle_start
        and float(row["elapsed_ms"]) <= idle_start + 15000.0
    ]

    def cpu_stats(selected):
        values = [float(row["cpu_percent"]) for row in selected]
        if not values:
            return {"samples": 0, "avg_cpu_percent": None, "max_cpu_percent": None}
        return {
            "samples": len(values),
            "avg_cpu_percent": sum(values) / len(values),
            "max_cpu_percent": max(values),
        }

    def cpu_time_stats(selected):
        if len(selected) < 2:
            return {
                "samples": len(selected),
                "elapsed_ms": None,
                "cpu_time_seconds": None,
                "cpu_percent_by_time": None,
            }
        first = selected[0]
        last = selected[-1]
        elapsed_seconds = (
            float(last["elapsed_ms"]) - float(first["elapsed_ms"])
        ) / 1000.0
        cpu_seconds = float(last["cpu_time_seconds"]) - float(first["cpu_time_seconds"])
        return {
            "samples": len(selected),
            "elapsed_ms": elapsed_seconds * 1000.0,
            "cpu_time_seconds": cpu_seconds,
            "cpu_percent_by_time": (cpu_seconds / elapsed_seconds * 100.0)
            if elapsed_seconds > 0.0
            else None,
        }

    def quiescent_after_ms():
        target = 1.0
        sustain_ms = 3000.0
        candidates = [
            row for row in rows if float(row["elapsed_ms"]) >= idle_start
        ]
        for index, row in enumerate(candidates):
            start = float(row["elapsed_ms"])
            window = [
                candidate
                for candidate in candidates[index:]
                if float(candidate["elapsed_ms"]) <= start + sustain_ms
            ]
            if not window or float(window[-1]["elapsed_ms"]) < start + sustain_ms - 250.0:
                continue
            stats = cpu_time_stats(window)
            if (
                stats["cpu_percent_by_time"] is not None
                and stats["cpu_percent_by_time"] < target
            ):
                return start - idle_start
        return None

    def rss_stats(selected):
        values = [int(row["rss_bytes"]) for row in selected]
        if not values:
            return {
                "samples": 0,
                "first_rss_bytes": None,
                "last_rss_bytes": None,
                "min_rss_bytes": None,
                "max_rss_bytes": None,
                "delta_rss_bytes": None,
            }
        return {
            "samples": len(values),
            "first_rss_bytes": values[0],
            "last_rss_bytes": values[-1],
            "min_rss_bytes": min(values),
            "max_rss_bytes": max(values),
            "delta_rss_bytes": values[-1] - values[0],
        }

    sample_summary = {
        "scenario": scenario,
        "time_to_idle_ms": idle_start,
        "all_samples": cpu_stats(rows),
        "idle_samples": cpu_stats(idle_rows),
        "memory": {
            "all_samples": rss_stats(rows),
            "idle_samples": rss_stats(idle_rows),
            "idle_tail_0_to_3s": rss_stats(idle_tail_rows),
            "quiescent_5_to_15s": rss_stats(quiescent_rows),
            "post_idle_0_to_15s_shutdown_excluded": rss_stats(shutdown_excluded_rows),
        },
        "idle_tail_0_to_3s": {
            "ps_cpu": cpu_stats(idle_tail_rows),
            "cpu_time": cpu_time_stats(idle_tail_rows),
        },
        "quiescent_5_to_15s": {
            "ps_cpu": cpu_stats(quiescent_rows),
            "cpu_time": cpu_time_stats(quiescent_rows),
        },
        "post_idle_0_to_15s_shutdown_excluded": {
            "ps_cpu": cpu_stats(shutdown_excluded_rows),
            "cpu_time": cpu_time_stats(shutdown_excluded_rows),
        },
        "time_to_quiescent_ms": quiescent_after_ms(),
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
