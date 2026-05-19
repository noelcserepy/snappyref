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

python3 - \
  "$binary" \
  "$scenario" \
  "$asset_dir" \
  "$run_dir" \
  "$library_root" \
  "$repo_root" <<'PY'
import csv
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import time

binary, scenario, asset_dir, run_dir, library_root, repo_root = sys.argv[1:]
run_dir = pathlib.Path(run_dir)
repo_root = pathlib.Path(repo_root)
samples_path = run_dir / "samples.csv"
samples_tree_path = run_dir / "samples-tree.csv"
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


def _load_sampling_module():
    module_path = repo_root / "scripts" / "_perf_sampling.py"
    spec = importlib.util.spec_from_file_location("_perf_sampling", module_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


sampling = _load_sampling_module()


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


def sample_main(pid, elapsed_ms):
    probe = subprocess.run(
        ["ps", "-p", str(pid), "-o", "pid=,%cpu=,time=,rss="],
        capture_output=True,
        text=True,
    )
    line = probe.stdout.strip()
    if not line:
        return None
    parts = line.split()
    if len(parts) < 4:
        return None
    return {
        "elapsed_ms": f"{elapsed_ms:.2f}",
        "pid": parts[0],
        "cpu_percent": parts[1],
        "cpu_time_seconds": str(parse_cpu_time(parts[2])),
        "rss_bytes": str(int(parts[3]) * 1024),
    }


def sample_tree(root_pid, elapsed_ms):
    """Sample the main process and its descendants.

    Uses `ps -axwwo pid,ppid,rss,%cpu,command` so the embedding helper
    and any future image preparation helper subprocess are captured at
    the same tick as the main app. Returns a list of per-pid dicts that
    the caller writes to ``samples-tree.csv`` and feeds back into the
    sampling module for tree peak / per-helper-peak summaries.
    """

    probe = subprocess.run(
        ["ps", "-axwwo", "pid=,ppid=,rss=,%cpu=,command="],
        capture_output=True,
        text=True,
    )
    rows = sampling.parse_ps_axwo_output(probe.stdout)
    descendants = sampling.select_descendants(rows, root_pid)
    out = []
    for row in descendants:
        out.append(
            {
                "elapsed_ms": elapsed_ms,
                "pid": row["pid"],
                "ppid": row["ppid"],
                "role": sampling.classify_helper(
                    row.get("command", ""), root_pid, row["pid"]
                ),
                "rss_bytes": row["rss_bytes"],
                "cpu_percent": row["cpu_percent"],
                "command": row.get("command", ""),
            }
        )
    return out


started = time.monotonic()
process = subprocess.Popen(cmd, env=env)
root_pid = process.pid
print(f"scenario={scenario} pid={root_pid} run_dir={run_dir}", flush=True)


def maybe_activate_app():
    if os.environ.get("SNAPPYREF_PERF_FRONTMOST") != "1":
        return
    if sys.platform != "darwin":
        return
    subprocess.run(
        [
            "osascript",
            "-e",
            'tell application "System Events" to set frontmost of process "snappyref" to true',
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )


maybe_activate_app()

tree_samples: list[dict] = []

with samples_path.open("w", newline="") as handle, samples_tree_path.open(
    "w", newline=""
) as tree_handle:
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
    tree_writer = csv.DictWriter(
        tree_handle,
        fieldnames=[
            "elapsed_ms",
            "pid",
            "ppid",
            "role",
            "cpu_percent",
            "rss_bytes",
            "command",
        ],
    )
    tree_writer.writeheader()
    while process.poll() is None:
        elapsed_ms = (time.monotonic() - started) * 1000.0
        main_sample = sample_main(root_pid, elapsed_ms)
        if main_sample is not None:
            writer.writerow(main_sample)
            handle.flush()
        # Tree sampling tracks the same tick so per-tick sums in
        # `_perf_sampling.tree_rss_stats` line up across pids.
        for tree_row in sample_tree(root_pid, elapsed_ms):
            tree_samples.append(tree_row)
            tree_writer.writerow(
                {
                    "elapsed_ms": f"{tree_row['elapsed_ms']:.2f}",
                    "pid": tree_row["pid"],
                    "ppid": tree_row["ppid"],
                    "role": tree_row["role"],
                    "cpu_percent": f"{tree_row['cpu_percent']}",
                    "rss_bytes": tree_row["rss_bytes"],
                    "command": tree_row["command"],
                }
            )
        tree_handle.flush()
        time.sleep(0.2)

returncode = process.wait()
summary_path = run_dir / "summary.json"
if summary_path.exists():
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

    # Whole-process-tree peak captures embedding-helper and image-prep
    # helper RSS so a faster main-process **Quiescent Memory** number
    # cannot quietly hide a regression in helper-side allocations.
    tree_during_active = [
        row for row in tree_samples if row["elapsed_ms"] < idle_start
    ] if idle_start > 0.0 else tree_samples
    tree_during_quiescent = [
        row
        for row in tree_samples
        if idle_start + 5000.0 <= row["elapsed_ms"] < idle_start + 15000.0
    ]

    process_tree = {
        "active_work": sampling.tree_rss_stats(tree_during_active),
        "quiescent_5_to_15s": sampling.tree_rss_stats(tree_during_quiescent),
        "all_samples": sampling.tree_rss_stats(tree_samples),
        "per_helper_active": sampling.per_helper_peaks(
            tree_during_active, main_pid=root_pid
        ),
        "per_helper_overall": sampling.per_helper_peaks(
            tree_samples, main_pid=root_pid
        ),
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
        "process_tree": process_tree,
    }
    (run_dir / "samples-summary.json").write_text(
        json.dumps(sample_summary, indent=2) + "\n"
    )

    # `phase-summary.json` is the new before/after-friendly artifact
    # that turns the raw `events.ndjson` into headline phase deltas and
    # per-format groups. Existing artifacts above are unchanged so older
    # tooling continues to work.
    events_path = run_dir / "events.ndjson"
    events = sampling.load_events_ndjson(events_path)
    phase_summary = {
        "scenario": scenario,
        "time_to_idle_ms": idle_start,
        "phase_summary": sampling.phase_summary_from_events(events),
        "per_format": sampling.per_format_groups_from_events(events),
        "process_tree_peak": {
            "active_work_peak_rss_bytes": process_tree["active_work"][
                "peak_tree_rss_bytes"
            ],
            "overall_peak_rss_bytes": process_tree["all_samples"][
                "peak_tree_rss_bytes"
            ],
            "main_quiescent_max_rss_bytes": sample_summary["memory"][
                "quiescent_5_to_15s"
            ]["max_rss_bytes"],
        },
    }
    (run_dir / "phase-summary.json").write_text(
        json.dumps(phase_summary, indent=2) + "\n"
    )
print(f"run_dir={run_dir}")
print(f"returncode={returncode}")
sys.exit(returncode)
PY
