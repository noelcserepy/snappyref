#!/usr/bin/env bash
#
# Measure cold-startup, settling, idle CPU, **Quiescent Memory**, and
# queued background work for libraries that already contain assets and
# placements when the app launches. Complements `run-perf-scenario.sh`
# (empty-library import flows) and `run-idle-board-size.sh` (board-size
# idle invariance with a single library asset).
#
# Each invocation runs through several cases. By default:
#
#   library-100         100 library assets, 0 placements on the active board
#   library-1000        1000 library assets, 0 placements on the active board
#   library-100-board-100   100 library assets, 100 placements on board 1
#   library-1000-board-1000 1000 library assets, 1000 placements on board 1
#
# The first two isolate **Library Size** scaling (search/backfill/asset
# load) from active-board cost. The third and fourth combine library
# size with **Board Size** scaling. Run together they let perf work
# distinguish board-size idle invariance from library-size scaling.
#
# Output layout (matches `run-perf-scenario.sh`):
#
#   perf-assets/runs/<timestamp>-existing-library-baseline/
#     seed-import-1/                  # one-asset seed used by all cases
#     case-library-100/
#       library/                      # populated library on disk
#       idle-samples.csv              # ps cpu/rss timeline
#       summary.json                  # scenario summary (matches existing perf shape)
#       samples-summary.json          # quiescent windows
#       background-state.json         # queued background work (assets, embeddings)
#     case-library-1000/...
#     existing-library-overall.json   # one file with every case rolled up
#
# Usage:
#
#     scripts/run-existing-library-baseline.sh
#     SNAPPYREF_EXISTING_LIBRARY_CASES="library-100 library-1000" \
#       scripts/run-existing-library-baseline.sh
#
# Env vars:
#   SNAPPYREF_PERF_BINARY                  - app binary (default: target/release/snappyref)
#   SNAPPYREF_PERF_ASSET_DIR               - perf corpus root (default: perf-assets/images)
#   SNAPPYREF_PERF_RUN_DIR                 - override run directory
#   SNAPPYREF_EXISTING_LIBRARY_CASES       - space-separated case names
#   SNAPPYREF_EXISTING_LIBRARY_STARTUP_WAIT_SECS  - seconds to wait before sampling (default 25)
#   SNAPPYREF_EXISTING_LIBRARY_SAMPLE_SECS - sample window seconds (default 30)
#   SNAPPYREF_EXISTING_LIBRARY_SAMPLE_INTERVAL_SECS - sampling interval (default 0.2)
#
# The script reuses `scripts/_perf_library_seed.py` for the library-size
# multiplication so the seeding logic is covered by `--self-test`.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
timestamp=$(date +"%Y%m%d-%H%M%S")
run_dir=${SNAPPYREF_PERF_RUN_DIR:-"$repo_root/perf-assets/runs/$timestamp-existing-library-baseline"}
asset_dir=${SNAPPYREF_PERF_ASSET_DIR:-"$repo_root/perf-assets/images"}
binary=${SNAPPYREF_PERF_BINARY:-"$repo_root/target/release/snappyref"}
cases=${SNAPPYREF_EXISTING_LIBRARY_CASES:-"library-100 library-1000 library-100-board-100 library-1000-board-1000"}
startup_wait_secs=${SNAPPYREF_EXISTING_LIBRARY_STARTUP_WAIT_SECS:-25}
sample_secs=${SNAPPYREF_EXISTING_LIBRARY_SAMPLE_SECS:-30}
sample_interval_secs=${SNAPPYREF_EXISTING_LIBRARY_SAMPLE_INTERVAL_SECS:-0.2}

mkdir -p "$run_dir"

cargo build --release

python3 - \
  "$binary" \
  "$asset_dir" \
  "$run_dir" \
  "$cases" \
  "$startup_wait_secs" \
  "$sample_secs" \
  "$sample_interval_secs" \
  "$repo_root" <<'PY'
import csv
import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import sys
import time

(
    binary,
    asset_dir,
    run_dir,
    cases_text,
    startup_wait_secs,
    sample_secs,
    sample_interval_secs,
    repo_root,
) = sys.argv[1:]
run_dir = pathlib.Path(run_dir)
repo_root = pathlib.Path(repo_root)
startup_wait_secs = float(startup_wait_secs)
sample_secs = float(sample_secs)
sample_interval_secs = float(sample_interval_secs)
cases = cases_text.split()
seed_run_dir = run_dir / "seed-import-1"
seed_library = seed_run_dir / "library"


def _load_module(name):
    module_path = repo_root / "scripts" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, module_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


seed_module = _load_module("_perf_library_seed")
sampling = _load_module("_perf_sampling")


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


def process_sample(pid):
    probe = subprocess.run(
        ["ps", "-p", str(pid), "-o", "pid=,%cpu=,time=,rss="],
        capture_output=True,
        text=True,
    )
    parts = probe.stdout.strip().split()
    if len(parts) < 4:
        return None
    return {
        "pid": parts[0],
        "cpu_percent": float(parts[1]),
        "cpu_time_seconds": parse_cpu_time(parts[2]),
        "rss_bytes": int(parts[3]) * 1024,
    }


def select_window(rows, start_ms, end_ms):
    return [
        row
        for row in rows
        if start_ms <= row["elapsed_ms"] < end_ms
    ]


def cpu_stats(rows):
    values = [row["cpu_percent"] for row in rows]
    if not values:
        return {"samples": 0, "avg_cpu_percent": None, "max_cpu_percent": None}
    return {
        "samples": len(values),
        "avg_cpu_percent": sum(values) / len(values),
        "max_cpu_percent": max(values),
    }


def cpu_time_stats(rows):
    if len(rows) < 2:
        return {
            "samples": len(rows),
            "elapsed_ms": None,
            "cpu_time_seconds": None,
            "cpu_percent_by_time": None,
        }
    first = rows[0]
    last = rows[-1]
    elapsed_seconds = (last["elapsed_ms"] - first["elapsed_ms"]) / 1000.0
    cpu_seconds = last["cpu_time_seconds"] - first["cpu_time_seconds"]
    return {
        "samples": len(rows),
        "elapsed_ms": elapsed_seconds * 1000.0,
        "cpu_time_seconds": cpu_seconds,
        "cpu_percent_by_time": (
            cpu_seconds / elapsed_seconds * 100.0 if elapsed_seconds > 0 else None
        ),
    }


def rss_stats(rows):
    values = [row["rss_bytes"] for row in rows]
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


def settling_marker(rows):
    """Return the elapsed_ms at which RSS first stops growing meaningfully.

    Used as a coarse, machine-derivable "startup/load settling" marker so
    the same run can report a startup-settled-ish elapsed time without
    having to embed scenario events in the cold-launch path. Returns
    ``None`` when the window is too short to tell.
    """
    if len(rows) < 5:
        return None
    growth_threshold = 1024 * 1024  # 1 MB
    sustain_samples = 5
    stable = 0
    last_rss = rows[0]["rss_bytes"]
    for row in rows:
        if abs(row["rss_bytes"] - last_rss) <= growth_threshold:
            stable += 1
            if stable >= sustain_samples:
                return row["elapsed_ms"]
        else:
            stable = 0
        last_rss = row["rss_bytes"]
    return None


def quiescent_after_ms(rows):
    """Time from sample start until CPU stays below 1% for 3 s."""
    target = 1.0
    sustain_ms = 3000.0
    for index, row in enumerate(rows):
        start = row["elapsed_ms"]
        window = [
            candidate
            for candidate in rows[index:]
            if candidate["elapsed_ms"] <= start + sustain_ms
        ]
        if not window or window[-1]["elapsed_ms"] < start + sustain_ms - 250.0:
            continue
        stats = cpu_time_stats(window)
        if (
            stats["cpu_percent_by_time"] is not None
            and stats["cpu_percent_by_time"] < target
        ):
            return start - rows[0]["elapsed_ms"]
    return None


def seed_library_once():
    if seed_library.exists():
        shutil.rmtree(seed_library)
    seed_run_dir.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["SNAPPYREF_LIBRARY_ROOT"] = str(seed_library)
    env["SNAPPYREF_PERF_IDLE_HOLD_SECS"] = "1"
    cmd = [
        binary,
        "--perf-scenario=import-1",
        f"--perf-corpus={asset_dir}",
        f"--perf-run-dir={seed_run_dir}",
        "--perf-log",
    ]
    result = subprocess.run(cmd, env=env)
    if result.returncode != 0:
        raise SystemExit(result.returncode)


def parse_case(name):
    """Translate a case name like 'library-100-board-100' to (assets, placements)."""
    parts = name.split("-")
    if len(parts) < 2 or parts[0] != "library":
        raise SystemExit(f"unknown case name: {name}")
    try:
        assets = int(parts[1])
    except ValueError as error:
        raise SystemExit(f"unknown case name: {name}") from error
    placements = 0
    if len(parts) >= 4 and parts[2] == "board":
        try:
            placements = int(parts[3])
        except ValueError as error:
            raise SystemExit(f"unknown case name: {name}") from error
    return assets, placements


def build_case_library(case_name):
    assets, placements = parse_case(case_name)
    case_dir = run_dir / f"case-{case_name}"
    case_dir.mkdir(parents=True, exist_ok=True)
    library = case_dir / "library"
    seed_module.seed_library(
        seed_library,
        library,
        asset_count=assets,
        placement_count=placements,
    )
    return case_dir, library, assets, placements


def tree_sample(root_pid, elapsed_ms):
    """Sample the main process and its descendants.

    Mirrors the tree sampler in `run-perf-scenario.sh` so the existing
    library baseline can also report **Embedding Helper Process** /
    **Image Preparation Helper Process** peak RSS during the
    library-load and **Embedding Backfill** windows.
    """

    probe = subprocess.run(
        ["ps", "-axwwo", "pid=,ppid=,rss=,%cpu=,command="],
        capture_output=True,
        text=True,
    )
    parsed = sampling.parse_ps_axwo_output(probe.stdout)
    out = []
    for row in sampling.select_descendants(parsed, root_pid):
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


def measure_case(case_dir, library):
    env = os.environ.copy()
    env["SNAPPYREF_LIBRARY_ROOT"] = str(library)
    with (case_dir / "app.log").open("w") as app_log:
        process = subprocess.Popen(
            [binary],
            env=env,
            stdout=app_log,
            stderr=subprocess.STDOUT,
        )
        maybe_activate_app()
        started = time.monotonic()
        rows = []
        tree_rows = []
        try:
            time.sleep(startup_wait_secs)
            end_at = time.monotonic() + sample_secs
            while time.monotonic() < end_at and process.poll() is None:
                elapsed_ms = (time.monotonic() - started) * 1000.0
                sample = process_sample(process.pid)
                if sample:
                    sample["elapsed_ms"] = elapsed_ms
                    rows.append(sample)
                tree_rows.extend(tree_sample(process.pid, elapsed_ms))
                time.sleep(sample_interval_secs)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    return rows, tree_rows, process.pid


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


def write_samples(case_dir, rows, tree_rows):
    csv_path = case_dir / "idle-samples.csv"
    with csv_path.open("w", newline="") as handle:
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
        for row in rows:
            writer.writerow(
                {
                    "elapsed_ms": f"{row['elapsed_ms']:.2f}",
                    "pid": row["pid"],
                    "cpu_percent": row["cpu_percent"],
                    "cpu_time_seconds": row["cpu_time_seconds"],
                    "rss_bytes": row["rss_bytes"],
                }
            )
    if tree_rows:
        tree_path = case_dir / "idle-samples-tree.csv"
        with tree_path.open("w", newline="") as handle:
            writer = csv.DictWriter(
                handle,
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
            writer.writeheader()
            for row in tree_rows:
                writer.writerow(
                    {
                        "elapsed_ms": f"{row['elapsed_ms']:.2f}",
                        "pid": row["pid"],
                        "ppid": row["ppid"],
                        "role": row["role"],
                        "cpu_percent": row["cpu_percent"],
                        "rss_bytes": row["rss_bytes"],
                        "command": row["command"],
                    }
                )


def build_summary(case_name, library, rows, tree_rows, main_pid, assets, placements):
    sample_start_ms = rows[0]["elapsed_ms"] if rows else None
    idle_tail_rows = (
        select_window(rows, sample_start_ms, sample_start_ms + 3000.0)
        if sample_start_ms is not None
        else []
    )
    quiescent_rows = (
        select_window(rows, sample_start_ms + 5000.0, sample_start_ms + 15000.0)
        if sample_start_ms is not None
        else []
    )

    settle_ms = settling_marker(rows)
    background = seed_module.background_state(library)

    # Tree-level peak captures the working set of any
    # **Embedding Helper Process** that wakes for **Embedding Backfill**
    # plus any **Image Preparation Helper Process** triggered by the
    # cold library load. Quiescent main-process RSS stays reported
    # separately above so a faster headline number cannot quietly hide
    # a helper-side regression.
    quiescent_tree_rows = (
        [
            row
            for row in tree_rows
            if sample_start_ms + 5000.0 <= row["elapsed_ms"] < sample_start_ms + 15000.0
        ]
        if sample_start_ms is not None
        else []
    )
    process_tree = {
        "all_samples": sampling.tree_rss_stats(tree_rows),
        "quiescent_5_to_15s": sampling.tree_rss_stats(quiescent_tree_rows),
        "per_helper_overall": sampling.per_helper_peaks(tree_rows, main_pid=main_pid),
    }

    return {
        "scenario": "existing-library-baseline",
        "case": case_name,
        "library": str(library),
        "library_assets": assets,
        "board_placements": placements,
        "startup_wait_seconds": startup_wait_secs,
        "sample_seconds": sample_secs,
        "rss_settle_elapsed_ms": settle_ms,
        "time_to_quiescent_ms": quiescent_after_ms(rows) if rows else None,
        "background_state": background,
        "queued_background_work_present": background[
            "assets_missing_embeddings"
        ] > 0,
        "samples": {
            "all": {
                "cpu": cpu_stats(rows),
                "cpu_by_time": cpu_time_stats(rows),
                "rss": rss_stats(rows),
            },
            "idle_tail_0_to_3s": {
                "cpu": cpu_stats(idle_tail_rows),
                "cpu_by_time": cpu_time_stats(idle_tail_rows),
                "rss": rss_stats(idle_tail_rows),
            },
            "quiescent_5_to_15s": {
                "cpu": cpu_stats(quiescent_rows),
                "cpu_by_time": cpu_time_stats(quiescent_rows),
                "rss": rss_stats(quiescent_rows),
            },
            "process_tree": process_tree,
        },
    }


def run():
    seed_library_once()
    overall = []
    for case_name in cases:
        case_dir, library, assets, placements = build_case_library(case_name)
        expectation = "empty active board expected" if placements == 0 else "placements should be visible"
        print(
            f"== case {case_name}: assets={assets} placements={placements} ({expectation}) ==",
            flush=True,
        )
        rows, tree_rows, main_pid = measure_case(case_dir, library)
        write_samples(case_dir, rows, tree_rows)
        summary = build_summary(
            case_name, library, rows, tree_rows, main_pid, assets, placements
        )
        (case_dir / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        (case_dir / "samples-summary.json").write_text(
            json.dumps(summary["samples"], indent=2) + "\n"
        )
        (case_dir / "background-state.json").write_text(
            json.dumps(summary["background_state"], indent=2) + "\n"
        )
        overall.append(summary)
        quiescent_cpu = summary["samples"]["quiescent_5_to_15s"]["cpu_by_time"][
            "cpu_percent_by_time"
        ]
        max_rss = summary["samples"]["quiescent_5_to_15s"]["rss"]["max_rss_bytes"]
        print(
            f"assets={assets} placements={placements} "
            f"quiescent_cpu_by_time={quiescent_cpu} "
            f"max_rss_quiescent={max_rss} "
            f"missing_embeddings={summary['background_state']['assets_missing_embeddings']}"
        )

    (run_dir / "existing-library-overall.json").write_text(
        json.dumps({"cases": overall}, indent=2) + "\n"
    )
    print(f"run_dir={run_dir}")


run()
PY
