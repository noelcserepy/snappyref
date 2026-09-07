"""Regression checks for evaluation validity and comparison decisions."""
import copy
import unittest
import tempfile
import json
import os
from pathlib import Path

from perf_eval import aggregate, compare, cpu_percent, cpu_seconds, validate_run, run_case, BUDGETS
from _perf_sampling import per_format_groups_from_events, classify_helper


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

    def test_repeated_file_imports_keep_separate_timings(self):
        events = []
        for queue, start, end in [(1, 0, 100), (2, 2000, 2050), (3, 4000, 4150)]:
            events += [dict(event='import_queued', queue_id=queue, file='a.jpg', format='jpg', elapsed_ms=start),
                       dict(event='placement_loaded', queue_id=queue, asset_id=queue, elapsed_ms=end)]
        group, = per_format_groups_from_events(events)
        self.assertEqual(group['count'], 3)
        self.assertEqual(group['import_latency_ms_mean'], 100)
        self.assertEqual(classify_helper('snappyref --imageprep --source=x', 1, 2), 'imageprep_helper')


if __name__ == '__main__':
    unittest.main()
