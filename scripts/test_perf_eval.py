"""Regression checks for evaluation validity and comparison decisions."""
import copy
import unittest
import tempfile
import json
import os
import signal
from unittest.mock import Mock, patch
from pathlib import Path

from perf_eval import aggregate, compare, cpu_percent, cpu_seconds, validate_run, run_case, BUDGETS, FAST_CASES, FAST_METRICS, cleanup_process_group
from typing import Any
from _perf_sampling import per_format_groups_from_events, classify_helper, parse_ps_axwo_output, select_descendants, peak_tree_rss_bytes, per_helper_peaks, phase_summary_from_events


class EvaluationTests(unittest.TestCase):
    def report(self):
        runs = [dict(case='import-1', valid=True, metrics={key: 1000 for key in BUDGETS}) for _ in range(3)]
        return dict(schema=1, valid=True, contract={'corpus': 'abc'}, cases=aggregate(runs))

    def test_comparison_accepts_noise_and_rejects_slowdown(self):
        before = self.report()
        after = copy.deepcopy(before)
        self.assertEqual(compare(before, after)['status'], 'pass')
        after['cases']['import-1']['metrics']['launch_to_idle_ms'] = dict(min=1150, median=1150, max=1150)
        self.assertEqual(compare(before, after)['status'], 'pass')
        after['cases']['import-1']['metrics']['launch_to_idle_ms'] = dict(min=1500, median=1500, max=1500)
        self.assertEqual(compare(before, after)['status'], 'regression')

    def test_overlapping_ranges_are_inconclusive_not_a_regression_or_pass(self):
        before = self.report()
        after = self.report()
        before['cases']['import-1']['metrics']['launch_to_idle_ms'] = dict(min=900, median=1000, max=1600)
        after['cases']['import-1']['metrics']['launch_to_idle_ms'] = dict(min=1000, median=1300, max=1700)
        result = compare(before, after)
        self.assertEqual(result['status'], 'inconclusive')
        self.assertFalse(any(change['status'] == 'regression' for change in result['changes']))

    def test_comparison_refuses_incomplete_or_different_experiments(self):
        before = self.report()
        for mutate in (
            lambda r: r.update(valid=False),
            lambda r: r['contract'].update(corpus='changed'),
            lambda r: r['cases']['import-1'].update(repeats=1),
            lambda r: r['cases']['import-1']['metrics'].pop('launch_to_idle_ms'),
            lambda r: r['cases'].clear(),
            lambda r: r['cases']['import-1']['metrics']['launch_to_idle_ms'].update(median=float('nan')),
        ):
            after = copy.deepcopy(before)
            mutate(after)
            with self.assertRaises(ValueError):
                compare(before, after)

    def test_comparison_refuses_missing_metrics_even_when_both_sides_match(self):
        report = self.report()
        report['cases']['import-1']['metrics'].pop('peak_tree_rss_bytes')
        with self.assertRaises(ValueError):
            compare(report, report)

    def fast_report(self):
        runs = [dict(case=case, repeat=repeat, valid=True,
                     metrics={key: 1000 for key in FAST_METRICS})
                for repeat in range(1, 4) for case in FAST_CASES]
        return dict(schema=1, valid=True,
                    contract=dict(profile='fast', cases=FAST_CASES, repeats=3),
                    runs=runs, cases=aggregate(runs, 'fast'))

    def test_fast_comparison_requires_complete_trials_and_matching_contract(self):
        before = self.fast_report()
        self.assertEqual(compare(before, before)['status'], 'pass')
        for mutate in (
            lambda r: r['runs'].pop(),
            lambda r: r['runs'][0].update(valid=False),
            lambda r: r['cases'].pop('board-1000'),
            lambda r: r['contract'].update(profile='full'),
            lambda r: r['contract'].update(external_stop_after_idle_ms=100),
        ):
            after = copy.deepcopy(before)
            mutate(after)
            with self.assertRaises(ValueError):
                compare(before, after)
        for trial in before['runs']:
            trial['metrics']['queue_to_all_visible_ms'] = None
        before['cases'] = aggregate(before['runs'], 'fast')
        with self.assertRaises(ValueError):
            compare(before, before)

    def test_fast_comparison_retains_regression_and_inconclusive_outcomes(self):
        before = self.fast_report()
        after = copy.deepcopy(before)
        for run in after['runs']:
            run['metrics']['launch_to_idle_ms'] = 1500
        after['cases'] = aggregate(after['runs'], 'fast')
        self.assertEqual(compare(before, after)['status'], 'regression')
        before['runs'][0]['metrics']['launch_to_idle_ms'] = 1600
        before['cases'] = aggregate(before['runs'], 'fast')
        self.assertTrue(any(c['status'] == 'inconclusive' for c in compare(before, after)['changes']))

    def test_fast_requires_current_completion_without_resource_window(self):
        summary, events, _, state = self.valid_inputs()
        summary['time_to_idle_ms'] = 1000
        for event in events:
            event['elapsed_ms'] = 1000 if event['event'] == 'settled_idle' else 0
        events[0]['unix_ms'] = 100000
        events.append(dict(event='all_placements_visible', elapsed_ms=500))
        validate_run(summary, events, [], state, (1, 1), 'fast')
        for mutate in (
            lambda s, e, d: e.append(dict(event='work_resumed', elapsed_ms=1100)),
            lambda s, e, d: s.update(time_to_idle_ms=2000),
            lambda s, e, d: s.update(demand_embeddings_done=0),
            lambda s, e, d: d.update(unfinished_embeddings=1),
            lambda s, e, d: e.pop(),
        ):
            copied = copy.deepcopy((summary, events, state))
            mutate(*copied)
            with self.assertRaises(ValueError):
                validate_run(copied[0], copied[1], [], copied[2], (1, 1), 'fast')

    def test_cpu_uses_elapsed_time_and_handles_ps_day_format(self):
        self.assertEqual(cpu_seconds('1-02:03:04.50'), 93784.5)
        self.assertAlmostEqual(cpu_percent([
            dict(elapsed_ms=1000, cpu_time_seconds=50),
            dict(elapsed_ms=11000, cpu_time_seconds=50.05),
        ]), 0.5)

    def valid_inputs(self):
        summary = dict(scenario='import-1', expected_imports=1, loaded_imports=1,
                       visible_imports=1, demand_embeddings_queued=1, demand_embeddings_done=1,
                       import_worker=dict(queued_startup=0, queued_interactive=0, queued_background=0, in_flight=0))
        events = [dict(event=name) for name in ['scenario_clock', 'scenario_started',
                  'first_frame_painted', 'first_placement_visible', 'settled_idle']]
        rows = [dict(elapsed_ms=n*200) for n in range(50)]
        state = dict(assets=1, placements=1, unfinished_embeddings=0, failed_embeddings=0)
        return summary, events, rows, state

    def test_validation_rejects_partial_work_and_truncated_sampling(self):
        validate_run(*self.valid_inputs(), expected=(1, 1))
        for mutate in (
            lambda s, e, r, d: s.update(loaded_imports=0),
            lambda s, e, r, d: s.update(visible_imports=0),
            lambda s, e, r, d: s.update(demand_embeddings_done=0),
            lambda s, e, r, d: s['import_worker'].update(in_flight=1),
            lambda s, e, r, d: e.pop(),
            lambda s, e, r, d: r.clear(),
            lambda s, e, r, d: d.update(unfinished_embeddings=1),
            lambda s, e, r, d: d.update(assets=2),
        ):
            inputs = self.valid_inputs()
            mutate(*inputs)
            with self.assertRaises(ValueError):
                validate_run(*inputs, expected=(1, 1))

    def test_timeout_records_failure_and_reaps_the_app(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            binary = directory / 'hung-app'
            binary.write_text("#!/usr/bin/env python3\nimport os, pathlib, time\n"
                              "pathlib.Path(os.environ['SNAPPYREF_DATA_ROOT']).write_text(str(os.getpid()))\n"
                              "time.sleep(60)\n")
            binary.chmod(0o755)
            pid_file = directory / 'pid'
            trial = directory / 'trial'
            with self.assertRaises(TimeoutError):
                run_case(binary, directory, trial, pid_file, 'import-1', timeout=0.5)
            self.assertFalse(json.loads((trial / 'result.json').read_text())['valid'])
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pid_file.read_text()), 0)

    def test_timeout_kills_a_helper_that_ignores_termination(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            binary = directory / 'hung-parent'
            binary.write_text("#!/usr/bin/env python3\nimport os, signal, time\n"
                              "child = os.fork()\n"
                              "if child == 0:\n    signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                              "time.sleep(60)\n")
            binary.chmod(0o755)
            trial = directory / 'trial'
            with self.assertRaises(TimeoutError):
                run_case(binary, directory, trial, directory, 'import-1', timeout=0.5, profile='fast')
            self.assertFalse(json.loads((trial / 'result.json').read_text())['valid'])
            self.assertGreater(json.loads((trial / 'timing.json').read_text())['cleanup_seconds'], 0)

    def test_cleanup_handles_exited_macos_groups_but_rejects_live_permission_errors(self):
        process = Mock(pid=12345)
        with patch('perf_eval.os.killpg', side_effect=PermissionError), patch(
                'perf_eval.live_process_group', return_value=False):
            cleanup_process_group(process)
        with patch('perf_eval.os.killpg', side_effect=PermissionError), patch(
                'perf_eval.live_process_group', return_value=True):
            with self.assertRaises(PermissionError):
                cleanup_process_group(process)
        with patch('perf_eval.os.killpg') as send, patch(
                'perf_eval.live_process_group', side_effect=[True, False]):
            cleanup_process_group(process)
            self.assertEqual(send.call_args_list[-1].args, (process.pid, signal.SIGKILL))

    def test_repeated_file_imports_keep_separate_timings(self):
        events = []
        for queue, start, end in [(1, 0, 100), (2, 2000, 2050), (3, 4000, 4150)]:
            events += [dict(event='import_queued', queue_id=queue, file='a.jpg', format='jpg', elapsed_ms=start),
                       dict(event='placement_loaded', queue_id=queue, asset_id=queue, elapsed_ms=end)]
        group, = per_format_groups_from_events(events)
        self.assertEqual(group['count'], 3)
        self.assertEqual(group['import_latency_ms_mean'], 100)


class SamplingTests(unittest.TestCase):
    def test_parse_ps_axwo_output(self) -> None:
        text = (
            "100 1 102400 1.5 /path/to/snappyref --perf-scenario=import-1\n"
            "200 100 51200 7.2 /path/to/snappyref --semantic-embed-server\n"
            "300 100 1024 0.0 /path/to/snappyref --imageprep\n"
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
            "300 100 25 0.0 /path/snappyref --imageprep\n"
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
            classify_helper("snappyref --imageprep", 100, 300),
            "imageprep_helper",
        )

    def _sample(self, *, elapsed_ms, pid, rss_bytes, cpu_percent, command) -> dict:
        return {
            "elapsed_ms": elapsed_ms,
            "pid": pid,
            "rss_bytes": rss_bytes,
            "cpu_percent": cpu_percent,
            "command": command,
        }

    def test_peak_tree_rss_bytes_sums_pids_per_tick(self) -> None:
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
        self.assertEqual(peak_tree_rss_bytes(rows), 500_000_000)

    def test_peak_tree_rss_bytes_handles_empty(self) -> None:
        self.assertIsNone(peak_tree_rss_bytes([]))

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
                command="snappyref --imageprep",
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
        self.assertEqual(peaks["imageprep_helper"]["peak_rss_bytes"], 40_000_000)

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


if __name__ == '__main__':
    unittest.main()
