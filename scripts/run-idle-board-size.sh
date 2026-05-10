#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
timestamp=$(date +"%Y%m%d-%H%M%S")
run_dir=${SNAPPYREF_PERF_RUN_DIR:-"$repo_root/perf-assets/runs/$timestamp-board-size-idle"}
asset_dir=${SNAPPYREF_PERF_ASSET_DIR:-"$repo_root/perf-assets/images"}
binary=${SNAPPYREF_PERF_BINARY:-"$repo_root/target/release/snappyref"}
sizes=${SNAPPYREF_IDLE_BOARD_SIZES:-"0 1 20 100 1000"}

mkdir -p "$run_dir"

cargo build --release

python3 - "$binary" "$asset_dir" "$run_dir" "$sizes" <<'PY'
import csv
import json
import os
import pathlib
import shutil
import sqlite3
import subprocess
import sys
import time

binary, asset_dir, run_dir, sizes_text = sys.argv[1:]
run_dir = pathlib.Path(run_dir)
seed_run_dir = run_dir / "seed-import-1"
seed_library = seed_run_dir / "library"
sizes = [int(value) for value in sizes_text.split()]


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


def stats(rows):
    cpu_values = [row["cpu_percent"] for row in rows]
    if len(rows) >= 2:
        elapsed_seconds = (rows[-1]["elapsed_ms"] - rows[0]["elapsed_ms"]) / 1000.0
        cpu_seconds = rows[-1]["cpu_time_seconds"] - rows[0]["cpu_time_seconds"]
        cpu_percent_by_time = (
            cpu_seconds / elapsed_seconds * 100.0 if elapsed_seconds > 0 else None
        )
    else:
        elapsed_seconds = None
        cpu_seconds = None
        cpu_percent_by_time = None
    return {
        "samples": len(rows),
        "avg_ps_cpu_percent": sum(cpu_values) / len(cpu_values) if cpu_values else None,
        "max_ps_cpu_percent": max(cpu_values) if cpu_values else None,
        "cpu_time_seconds": cpu_seconds,
        "cpu_percent_by_time": cpu_percent_by_time,
        "elapsed_ms": elapsed_seconds * 1000.0 if elapsed_seconds is not None else None,
        "max_rss_bytes": max((row["rss_bytes"] for row in rows), default=None),
        "final_rss_bytes": rows[-1]["rss_bytes"] if rows else None,
    }


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


def make_library(size):
    case_dir = run_dir / f"board-{size}"
    library = case_dir / "library"
    if library.exists():
        shutil.rmtree(library)
    shutil.copytree(seed_library, library)
    db = library / "library.db"
    with sqlite3.connect(db) as connection:
        connection.execute("DELETE FROM canvas_stack_items")
        connection.execute("DELETE FROM board_items")
        for index in range(size):
            item_id = index + 1
            x = float((index % 50) * 220)
            y = float((index // 50) * 180)
            scale = 0.0859152153134346
            connection.execute(
                """
                INSERT INTO board_items (
                    id, board_id, asset_id, x, y, scale, rotation, z_index, archived
                ) VALUES (?1, 1, 1, ?2, ?3, ?4, 0.0, ?5, 0)
                """,
                (item_id, x, y, scale, index),
            )
            connection.execute(
                """
                INSERT INTO canvas_stack_items (
                    board_id, item_kind, item_id, stack_index
                ) VALUES (1, 'placement', ?1, ?2)
                """,
                (item_id, index),
            )
    return case_dir, library


def measure_idle(size, library):
    env = os.environ.copy()
    env["SNAPPYREF_LIBRARY_ROOT"] = str(library)
    process = subprocess.Popen([binary], env=env)
    started = time.monotonic()
    startup_wait = float(os.environ.get("SNAPPYREF_IDLE_BOARD_STARTUP_WAIT_SECS", "15"))
    sample_secs = float(os.environ.get("SNAPPYREF_IDLE_BOARD_SAMPLE_SECS", "12"))
    interval = float(os.environ.get("SNAPPYREF_IDLE_BOARD_SAMPLE_INTERVAL_SECS", "0.2"))
    time.sleep(startup_wait)
    rows = []
    end_at = time.monotonic() + sample_secs
    while time.monotonic() < end_at and process.poll() is None:
        sample = process_sample(process.pid)
        if sample:
            sample["elapsed_ms"] = (time.monotonic() - started) * 1000.0
            rows.append(sample)
        time.sleep(interval)
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    return rows


seed_library_once()
overall = {}
for size in sizes:
    case_dir, library = make_library(size)
    rows = measure_idle(size, library)
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
        writer.writerows(rows)
    summary = {
        "placements": size,
            "startup_wait_seconds": float(
            os.environ.get("SNAPPYREF_IDLE_BOARD_STARTUP_WAIT_SECS", "15")
        ),
        "sample_seconds": float(os.environ.get("SNAPPYREF_IDLE_BOARD_SAMPLE_SECS", "12")),
        "idle": stats(rows),
    }
    (case_dir / "idle-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    overall[str(size)] = summary
    print(
        f"placements={size} "
        f"cpu_by_time={summary['idle']['cpu_percent_by_time']} "
        f"avg_ps={summary['idle']['avg_ps_cpu_percent']} "
        f"max_ps={summary['idle']['max_ps_cpu_percent']}"
    )

(run_dir / "board-size-summary.json").write_text(json.dumps(overall, indent=2) + "\n")
print(f"run_dir={run_dir}")
PY
