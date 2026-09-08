#!/usr/bin/env python3
"""Process-tree sampling and phase/format summaries for perf_eval.py."""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
import unittest
from collections import defaultdict
from typing import Any


# ---------------------------------------------------------------------------
# process-tree discovery
# ---------------------------------------------------------------------------


def parse_ps_axwo_output(text: str) -> list[dict[str, Any]]:
    """Parse the output of ``ps -axwwo pid=,ppid=,rss=,%cpu=,command=``.

    Returns a list of dicts with keys ``pid``, ``ppid``, ``rss_bytes``,
    ``cpu_percent``, and ``command``. The ``command`` column captures
    the full command line which lets the caller tell apart helper roles
    (the embedding helper reruns the app binary with
    ``--semantic-embed-server``).
    """

    rows: list[dict[str, Any]] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        parts = line.split(None, 4)
        if len(parts) < 5:
            # Some lines (zombies, kernel rows) can lack a command; skip them.
            continue
        try:
            pid = int(parts[0])
            ppid = int(parts[1])
            rss_kb = int(parts[2])
            cpu_percent = float(parts[3])
        except ValueError:
            continue
        rows.append(
            {
                "pid": pid,
                "ppid": ppid,
                "rss_bytes": rss_kb * 1024,
                "cpu_percent": cpu_percent,
                "command": parts[4],
            }
        )
    return rows


def select_descendants(rows: list[dict[str, Any]], root_pid: int) -> list[dict[str, Any]]:
    """Return the rows belonging to ``root_pid`` and any descendants.

    Tree walk is bounded by the input row count so unrelated grandchild
    chains do not pull in arbitrary system processes.
    """

    by_ppid: dict[int, list[dict[str, Any]]] = defaultdict(list)
    by_pid: dict[int, dict[str, Any]] = {}
    for row in rows:
        by_ppid[row["ppid"]].append(row)
        by_pid[row["pid"]] = row
    if root_pid not in by_pid:
        return []
    selected: dict[int, dict[str, Any]] = {root_pid: by_pid[root_pid]}
    frontier = [root_pid]
    # Hard cap on iterations: |rows| is at most the system process count
    # and we never add a pid twice.
    while frontier:
        next_frontier: list[int] = []
        for pid in frontier:
            for child in by_ppid.get(pid, []):
                if child["pid"] in selected:
                    continue
                selected[child["pid"]] = child
                next_frontier.append(child["pid"])
        frontier = next_frontier
    return list(selected.values())


# ---------------------------------------------------------------------------
# tree memory and CPU aggregates
# ---------------------------------------------------------------------------


def classify_helper(command: str, main_pid: int, pid: int) -> str:
    """Identify the main app and its shipping helper commands."""

    if pid == main_pid:
        return "main"
    if "--semantic-embed-server" in command:
        return "embedding_helper"
    if "--imageprep" in command:
        return "imageprep_helper"
    return "other_helper"


def tree_rss_stats(rows: list[dict[str, Any]]) -> dict[str, Any]:
    """Aggregate per-pid samples into a process-tree summary.

    Input rows have fields ``elapsed_ms``, ``role``, ``rss_bytes``,
    ``cpu_percent``. The same elapsed timestamp may appear once per pid
    for one sampling tick. The peak-tree RSS is the maximum, across
    sampling ticks, of the **sum** of RSS bytes for all pids in that
    tick. This is the meaningful "whole-process working set" peak that
    main-process-only sampling misses when helpers do work the main
    process never paged in.
    """

    if not rows:
        return {
            "samples": 0,
            "tick_count": 0,
            "peak_tree_rss_bytes": None,
            "peak_tree_cpu_percent": None,
        }

    by_tick: dict[float, dict[str, float]] = defaultdict(
        lambda: {"rss": 0.0, "cpu": 0.0, "pids": 0}
    )
    for row in rows:
        tick = float(row["elapsed_ms"])
        by_tick[tick]["rss"] += int(row["rss_bytes"])
        by_tick[tick]["cpu"] += float(row["cpu_percent"])
        by_tick[tick]["pids"] += 1

    rss_totals = [entry["rss"] for entry in by_tick.values()]
    cpu_totals = [entry["cpu"] for entry in by_tick.values()]
    return {
        "samples": len(rows),
        "tick_count": len(by_tick),
        "peak_tree_rss_bytes": int(max(rss_totals)),
        "peak_tree_cpu_percent": max(cpu_totals),
    }


def per_helper_peaks(
    rows: list[dict[str, Any]], main_pid: int
) -> dict[str, dict[str, Any]]:
    """Compute peak RSS and CPU per helper role.

    The peak is taken across all samples for that role; this is the
    headline number for "did the embedding helper or image preparation
    helper allocate something huge that main-process sampling missed".
    """

    by_role: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        role = classify_helper(row.get("command", ""), main_pid, int(row["pid"]))
        by_role[role].append(row)

    out: dict[str, dict[str, Any]] = {}
    for role, samples in by_role.items():
        rss_values = [int(row["rss_bytes"]) for row in samples]
        cpu_values = [float(row["cpu_percent"]) for row in samples]
        # First-seen / last-seen elapsed_ms keep helper lifetimes
        # legible when scanning the summary by eye.
        elapsed_values = [float(row["elapsed_ms"]) for row in samples]
        out[role] = {
            "samples": len(samples),
            "distinct_pids": sorted({int(row["pid"]) for row in samples}),
            "peak_rss_bytes": max(rss_values) if rss_values else None,
            "peak_cpu_percent": max(cpu_values) if cpu_values else None,
            "first_seen_ms": min(elapsed_values) if elapsed_values else None,
            "last_seen_ms": max(elapsed_values) if elapsed_values else None,
        }
    return out


# ---------------------------------------------------------------------------
# phase summary
# ---------------------------------------------------------------------------


# Phase boundary events emitted by `src/app/perf_scenario.rs`. Each pair maps to the
# elapsed_ms reading on the corresponding event; missing events are left
# as None so before/after comparisons fail loudly rather than quietly
# averaging to zero.
_PHASE_MARKERS = [
    "scenario_started",
    "first_placement_loaded",
    "all_placements_loaded",
    "first_placement_visible",
    "all_placements_visible",
    "semantic_model_load_started",
    "semantic_model_ready",
    "first_import_embedding_started",
    "first_import_embedding_done",
    "all_import_embeddings_done",
    "semantic_model_unload_requested",
    "semantic_model_unloaded",
    "settled_idle",
]


def phase_summary_from_events(events: list[dict[str, Any]]) -> dict[str, Any]:
    """Roll per-asset events into a phase-timing summary.

    Outputs the elapsed_ms of each headline phase marker plus durations
    between adjacent markers, so before/after diffs can attribute a
    Time-to-Idle change to a specific phase without re-parsing
    `events.ndjson` by hand.
    """

    elapsed_by_event: dict[str, float] = {}
    for event in events:
        kind = event.get("event")
        if not isinstance(kind, str):
            continue
        elapsed = event.get("elapsed_ms")
        if not isinstance(elapsed, (int, float)):
            continue
        # We only want the first occurrence of each marker; later
        # `all_import_embeddings_done` events would shadow the first.
        elapsed_by_event.setdefault(kind, float(elapsed))

    markers: dict[str, float | None] = {
        name: elapsed_by_event.get(name) for name in _PHASE_MARKERS
    }

    durations: dict[str, float | None] = {}

    def _delta(start: str, end: str) -> float | None:
        a = markers.get(start)
        b = markers.get(end)
        if a is None or b is None:
            return None
        return float(b) - float(a)

    durations["queue_to_first_placement_ms"] = _delta(
        "scenario_started", "first_placement_visible"
    )
    durations["queue_to_first_loaded_ms"] = _delta(
        "scenario_started", "first_placement_loaded"
    )
    durations["first_loaded_to_first_visible_ms"] = _delta(
        "first_placement_loaded", "first_placement_visible"
    )
    durations["all_loaded_to_all_visible_ms"] = _delta(
        "all_placements_loaded", "all_placements_visible"
    )
    durations["first_to_all_placements_ms"] = _delta(
        "first_placement_visible", "all_placements_visible"
    )
    durations["model_load_ms"] = _delta(
        "semantic_model_load_started", "semantic_model_ready"
    )
    durations["model_ready_to_first_embedding_ms"] = _delta(
        "semantic_model_ready", "first_import_embedding_started"
    )
    durations["first_embedding_ms"] = _delta(
        "first_import_embedding_started", "first_import_embedding_done"
    )
    durations["embedding_drain_ms"] = _delta(
        "first_import_embedding_started", "all_import_embeddings_done"
    )
    durations["model_unload_ms"] = _delta(
        "semantic_model_unload_requested", "semantic_model_unloaded"
    )
    durations["all_placements_to_settled_idle_ms"] = _delta(
        "all_placements_visible", "settled_idle"
    )
    durations["all_embeddings_to_settled_idle_ms"] = _delta(
        "all_import_embeddings_done", "settled_idle"
    )

    return {
        "markers_ms": markers,
        "durations_ms": durations,
    }


# ---------------------------------------------------------------------------
# per-format groups
# ---------------------------------------------------------------------------


def per_format_groups_from_events(events: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Group per-asset events by image format.

    Returns the completed evaluator format breakdown so a
    saved `phase-summary.json` can be diffed directly against a previous
    run without re-running the script.
    """

    rows = {}
    asset_rows = defaultdict(list)
    for event in events:
        kind = event.get("event")
        queue_id = event.get("queue_id")
        if kind == "import_queued":
            if queue_id in rows:
                raise ValueError(f"duplicate import queue id {queue_id}")
            rows[queue_id] = dict(file=event["file"], format=event["format"],
                                  queued_ms=event["elapsed_ms"], placement_ms=None,
                                  embedding_queued_ms=None, embedding_done_ms=None)
        elif kind == "placement_loaded" and queue_id in rows:
            row = rows[queue_id]
            row["placement_ms"] = event["elapsed_ms"]
            asset_rows[event["asset_id"]].append(row)
        elif kind in ("import_embedding_queued", "import_embedding_done"):
            key = "embedding_queued_ms" if kind.endswith("queued") else "embedding_done_ms"
            # One embedding can satisfy several placements of the same asset.
            # A later trickle request must not rewrite the earlier duration.
            for row in asset_rows[event["asset_id"]]:
                if row[key] is None:
                    row[key] = event["elapsed_ms"]

    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows.values():
        fmt = row.get("format") or "unknown"
        groups[fmt].append(row)

    summary: list[dict[str, Any]] = []
    for fmt, group in sorted(groups.items()):
        import_latencies = [
            (row["placement_ms"] - row["queued_ms"])
            for row in group
            if row.get("placement_ms") is not None and row.get("queued_ms") is not None
        ]
        embedding_latencies = [
            (row["embedding_done_ms"] - row["embedding_queued_ms"])
            for row in group
            if row.get("embedding_done_ms") is not None
            and row.get("embedding_queued_ms") is not None
        ]
        summary.append(
            {
                "format": fmt,
                "count": len(group),
                "import_latency_ms_mean": (
                    sum(import_latencies) / len(import_latencies)
                    if import_latencies
                    else None
                ),
                "import_latency_ms_max": (
                    max(import_latencies) if import_latencies else None
                ),
                "import_latency_ms_min": (
                    min(import_latencies) if import_latencies else None
                ),
                "embedding_latency_ms_mean": (
                    sum(embedding_latencies) / len(embedding_latencies)
                    if embedding_latencies
                    else None
                ),
                "embedding_latency_ms_max": (
                    max(embedding_latencies) if embedding_latencies else None
                ),
            }
        )
    return summary


def load_events_ndjson(path: pathlib.Path) -> list[dict[str, Any]]:
    """Read a live event stream; completed-run validation parses it strictly."""

    if not path.exists():
        return []
    out: list[dict[str, Any]] = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


# ---------------------------------------------------------------------------
# self-test
# ---------------------------------------------------------------------------


class _Tests(unittest.TestCase):
    def test_parse_ps_axwo_output(self) -> None:
        text = (
            "100 1 102400 1.5 /path/to/snappyref --perf-scenario=import-1\n"
            "200 100 51200 7.2 /path/to/snappyref --semantic-embed-server\n"
            "300 100 1024 0.0 /path/to/snappyref --image-prep-helper\n"
        )
        rows = parse_ps_axwo_output(text)
        self.assertEqual(len(rows), 3)
        self.assertEqual(rows[0]["pid"], 100)
        self.assertEqual(rows[0]["ppid"], 1)
        self.assertEqual(rows[0]["rss_bytes"], 102400 * 1024)
        self.assertAlmostEqual(rows[0]["cpu_percent"], 1.5)
        self.assertIn("--perf-scenario", rows[0]["command"])

    def test_select_descendants_returns_main_and_children(self) -> None:
        rows = parse_ps_axwo_output(
            "100 1 100 0.0 /path/snappyref\n"
            "200 100 50 0.0 /path/snappyref --semantic-embed-server\n"
            "300 100 25 0.0 /path/snappyref --image-prep\n"
            "400 999 12 0.0 /unrelated/process\n"
            "500 200 8 0.0 grandchild\n"
        )
        pids = {row["pid"] for row in select_descendants(rows, 100)}
        self.assertEqual(pids, {100, 200, 300, 500})

    def test_select_descendants_unknown_root_returns_empty(self) -> None:
        rows = parse_ps_axwo_output("100 1 100 0.0 main\n")
        self.assertEqual(select_descendants(rows, 999), [])

    def test_classify_helper(self) -> None:
        self.assertEqual(classify_helper("snappyref", 100, 100), "main")
        self.assertEqual(
            classify_helper("snappyref --semantic-embed-server", 100, 200),
            "embedding_helper",
        )
        self.assertEqual(
            classify_helper("snappyref --image-prep", 100, 300),
            "other_helper",
        )

    def _sample(self, *, elapsed_ms, pid, rss_bytes, cpu_percent, command) -> dict:
        return {
            "elapsed_ms": elapsed_ms,
            "pid": pid,
            "rss_bytes": rss_bytes,
            "cpu_percent": cpu_percent,
            "command": command,
        }

    def test_tree_rss_stats_sums_pids_per_tick(self) -> None:
        rows = [
            self._sample(
                elapsed_ms=100.0,
                pid=100,
                rss_bytes=200_000_000,
                cpu_percent=5.0,
                command="snappyref",
            ),
            self._sample(
                elapsed_ms=100.0,
                pid=200,
                rss_bytes=300_000_000,
                cpu_percent=80.0,
                command="snappyref --semantic-embed-server",
            ),
            self._sample(
                elapsed_ms=200.0,
                pid=100,
                rss_bytes=210_000_000,
                cpu_percent=2.0,
                command="snappyref",
            ),
            # Helper exits before tick 200, so its row is absent. Peak
            # must still account for tick 100's 500 MB.
        ]
        stats = tree_rss_stats(rows)
        self.assertEqual(stats["tick_count"], 2)
        self.assertEqual(stats["peak_tree_rss_bytes"], 500_000_000)
        self.assertAlmostEqual(stats["peak_tree_cpu_percent"], 85.0)

    def test_tree_rss_stats_handles_empty(self) -> None:
        stats = tree_rss_stats([])
        self.assertEqual(stats["samples"], 0)
        self.assertEqual(stats["tick_count"], 0)
        self.assertIsNone(stats["peak_tree_rss_bytes"])

    def test_per_helper_peaks_picks_per_role(self) -> None:
        rows = [
            self._sample(
                elapsed_ms=100.0,
                pid=100,
                rss_bytes=200_000_000,
                cpu_percent=5.0,
                command="snappyref",
            ),
            self._sample(
                elapsed_ms=100.0,
                pid=200,
                rss_bytes=300_000_000,
                cpu_percent=80.0,
                command="snappyref --semantic-embed-server",
            ),
            self._sample(
                elapsed_ms=300.0,
                pid=200,
                rss_bytes=350_000_000,
                cpu_percent=90.0,
                command="snappyref --semantic-embed-server",
            ),
            self._sample(
                elapsed_ms=400.0,
                pid=300,
                rss_bytes=40_000_000,
                cpu_percent=10.0,
                command="snappyref --image-prep",
            ),
        ]
        peaks = per_helper_peaks(rows, main_pid=100)
        self.assertEqual(peaks["main"]["peak_rss_bytes"], 200_000_000)
        self.assertEqual(
            peaks["embedding_helper"]["peak_rss_bytes"], 350_000_000
        )
        self.assertAlmostEqual(
            peaks["embedding_helper"]["peak_cpu_percent"], 90.0
        )
        self.assertEqual(peaks["embedding_helper"]["distinct_pids"], [200])
        self.assertEqual(peaks["embedding_helper"]["first_seen_ms"], 100.0)
        self.assertEqual(peaks["embedding_helper"]["last_seen_ms"], 300.0)
        self.assertEqual(peaks["other_helper"]["peak_rss_bytes"], 40_000_000)

    def _events(self) -> list[dict[str, Any]]:
        return [
            {"event": "scenario_started", "elapsed_ms": 0.0},
            {
                "event": "import_queued",
                "elapsed_ms": 5.0,
                "file": "jpg/a.jpg",
                "queue_id": 1,
                "format": "jpg",
                "bytes": 1000,
            },
            {
                "event": "import_queued",
                "elapsed_ms": 6.0,
                "file": "png/b.png",
                "queue_id": 2,
                "format": "png",
                "bytes": 2000,
            },
            {
                "event": "placement_loaded",
                "elapsed_ms": 50.0,
                "filename": "999-0-a.jpg",
                "queue_id": 1,
                "asset_id": 1,
            },
            {"event": "first_placement_loaded", "elapsed_ms": 50.0},
            {
                "event": "placement_loaded",
                "elapsed_ms": 200.0,
                "filename": "999-1-b.png",
                "queue_id": 2,
                "asset_id": 2,
            },
            {"event": "all_placements_loaded", "elapsed_ms": 200.0},
            {"event": "first_placement_visible", "elapsed_ms": 75.0},
            {"event": "all_placements_visible", "elapsed_ms": 230.0},
            {"event": "semantic_model_load_started", "elapsed_ms": 60.0},
            {"event": "semantic_model_ready", "elapsed_ms": 360.0},
            {"event": "first_import_embedding_started", "elapsed_ms": 365.0},
            {"event": "first_import_embedding_done", "elapsed_ms": 600.0},
            {
                "event": "import_embedding_queued",
                "elapsed_ms": 51.0,
                "asset_id": 1,
            },
            {
                "event": "import_embedding_queued",
                "elapsed_ms": 201.0,
                "asset_id": 2,
            },
            {
                "event": "import_embedding_done",
                "elapsed_ms": 600.0,
                "asset_id": 1,
            },
            {
                "event": "import_embedding_done",
                "elapsed_ms": 700.0,
                "asset_id": 2,
            },
            {"event": "all_import_embeddings_done", "elapsed_ms": 700.0},
            {"event": "semantic_model_unload_requested", "elapsed_ms": 705.0},
            {"event": "semantic_model_unloaded", "elapsed_ms": 720.0},
            {"event": "settled_idle", "elapsed_ms": 730.0},
        ]

    def test_phase_summary_picks_markers_and_durations(self) -> None:
        summary = phase_summary_from_events(self._events())
        markers = summary["markers_ms"]
        durations = summary["durations_ms"]
        self.assertEqual(markers["scenario_started"], 0.0)
        self.assertEqual(markers["settled_idle"], 730.0)
        self.assertAlmostEqual(durations["queue_to_first_placement_ms"], 75.0)
        self.assertAlmostEqual(durations["queue_to_first_loaded_ms"], 50.0)
        self.assertAlmostEqual(durations["first_loaded_to_first_visible_ms"], 25.0)
        self.assertAlmostEqual(durations["all_loaded_to_all_visible_ms"], 30.0)
        self.assertAlmostEqual(durations["model_load_ms"], 300.0)
        self.assertAlmostEqual(durations["first_embedding_ms"], 235.0)
        self.assertAlmostEqual(durations["embedding_drain_ms"], 335.0)
        self.assertAlmostEqual(durations["model_unload_ms"], 15.0)
        self.assertAlmostEqual(durations["all_placements_to_settled_idle_ms"], 500.0)

    def test_phase_summary_missing_markers_yield_none(self) -> None:
        events = [event for event in self._events() if event["event"] != "settled_idle"]
        summary = phase_summary_from_events(events)
        self.assertIsNone(summary["markers_ms"]["settled_idle"])
        self.assertIsNone(summary["durations_ms"]["all_placements_to_settled_idle_ms"])

    def test_per_format_groups_join_events(self) -> None:
        groups = per_format_groups_from_events(self._events())
        by_format = {entry["format"]: entry for entry in groups}
        self.assertEqual(by_format["jpg"]["count"], 1)
        self.assertEqual(by_format["png"]["count"], 1)
        self.assertAlmostEqual(by_format["jpg"]["import_latency_ms_mean"], 45.0)
        self.assertAlmostEqual(by_format["png"]["import_latency_ms_mean"], 194.0)
        self.assertAlmostEqual(by_format["jpg"]["embedding_latency_ms_mean"], 549.0)


def _self_test() -> int:
    loader = unittest.TestLoader()
    suite = loader.loadTestsFromTestCase(_Tests)
    runner = unittest.TextTestRunner(verbosity=2)
    result = runner.run(suite)
    return 0 if result.wasSuccessful() else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="run unit tests")
    parser.add_argument(
        "--phase-summary",
        type=pathlib.Path,
        help="path to events.ndjson; prints the phase summary JSON",
    )
    args = parser.parse_args(argv)
    if args.self_test:
        return _self_test()
    if args.phase_summary:
        events = load_events_ndjson(args.phase_summary)
        out = {
            "phase_summary": phase_summary_from_events(events),
            "per_format": per_format_groups_from_events(events),
        }
        sys.stdout.write(json.dumps(out, indent=2) + "\n")
        return 0
    parser.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
