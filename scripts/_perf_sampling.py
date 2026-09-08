#!/usr/bin/env python3
"""Process-tree sampling and phase/format summaries for perf_eval.py."""

from __future__ import annotations

import json
import pathlib
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


def peak_tree_rss_bytes(rows: list[dict[str, Any]]) -> int | None:
    """Maximum per-tick sum of main and helper RSS, unchanged by helper exits."""
    by_tick: dict[float, int] = defaultdict(int)
    for row in rows:
        by_tick[float(row["elapsed_ms"])] += int(row["rss_bytes"])
    return max(by_tick.values(), default=None)


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

    Included in each trial result alongside phase timing.
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
