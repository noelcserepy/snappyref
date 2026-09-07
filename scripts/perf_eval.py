#!/usr/bin/env python3
"""Release-app performance evaluation. Run --help for suites and comparisons."""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import signal
import sqlite3
import fcntl
import statistics
import subprocess
import sys
import time

import _perf_sampling as sampling
from _perf_library_seed import background_state, copy_file, seed_library

ROOT = Path(__file__).resolve().parent.parent
IMPORTS = ['import-1', 'import-20', 'trickle-import', 'format:png', 'animated',
           'animated-control', 'static-20']
BOARDS = [f'board-{n}' for n in (0, 1, 20, 100, 1000)]
LIBRARIES = ['library-100', 'library-1000', 'library-100-board-100',
             'library-1000-board-1000', 'library-20-backfill']
SUITES = dict(core=IMPORTS, boards=BOARDS, libraries=LIBRARIES,
              full=IMPORTS + BOARDS + LIBRARIES)
# Both relative and absolute allowances must be exceeded to flag a regression.
BUDGETS = {
    'launch_to_first_frame_ms': (0.20, 100),
    'launch_to_first_visible_ms': (0.20, 100),
    'queue_to_all_visible_ms': (0.20, 100),
    'launch_to_idle_ms': (0.15, 200),
    'quiescent_cpu_percent': (0.50, 0.3),
    'quiescent_rss_bytes': (0.10, 16 * 1024**2),
    'peak_tree_rss_bytes': (0.10, 32 * 1024**2),
}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def output(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def fingerprint(paths):
    digest = hashlib.sha256()
    for label, path in sorted(paths):
        digest.update(label.encode())
        with path.open('rb') as handle:
            for block in iter(lambda: handle.read(1024**2), b''):
                digest.update(block)
    return digest.hexdigest()


def cpu_seconds(value):
    days, sep, clock = value.partition('-')
    seconds = int(days) * 86400 if sep else 0
    parts = (clock if sep else value).split(':')
    return seconds + sum(float(part) * 60**index for index, part in enumerate(reversed(parts)))


def cpu_percent(rows):
    if len(rows) < 2:
        raise ValueError('insufficient CPU samples')
    elapsed = (rows[-1]['elapsed_ms'] - rows[0]['elapsed_ms']) / 1000
    delta = rows[-1]['cpu_time_seconds'] - rows[0]['cpu_time_seconds']
    if elapsed <= 0 or delta < 0:
        raise ValueError('invalid CPU time delta')
    return delta / elapsed * 100


def validate_run(summary, events, rows, state, expected):
    markers = {event['event']: event for event in events}
    for name in ('scenario_clock', 'scenario_started', 'first_frame_painted', 'settled_idle'):
        if name not in markers:
            raise ValueError(f'missing {name}')
    count = summary['expected_imports']
    if summary['loaded_imports'] != count:
        raise ValueError('incomplete placement loading')
    if count and ('first_placement_visible' not in markers or summary['visible_imports'] < 1):
        raise ValueError('no painted placements')
    if summary['scenario'] != 'startup' and summary['visible_imports'] != count:
        raise ValueError('incomplete painted imports')
    if summary['demand_embeddings_queued'] != summary['demand_embeddings_done']:
        raise ValueError('incomplete demand embeddings')
    if any(summary['import_worker'].values()):
        raise ValueError('import workers have pending work')
    if state['unfinished_embeddings'] or state['failed_embeddings']:
        raise ValueError(f'incomplete embeddings: {state}')
    if expected and (state['assets'], state['placements']) != expected:
        raise ValueError(f'fixture cardinality changed: wanted {expected}, got {state}')
    if state['placements'] != count:
        raise ValueError('persisted placement count disagrees with scenario')
    if len(rows) < 30 or rows[-1]['elapsed_ms'] - rows[0]['elapsed_ms'] < 9000:
        raise ValueError('quiescent window is incomplete')
    if max(b['elapsed_ms'] - a['elapsed_ms'] for a, b in zip(rows, rows[1:])) > 1000:
        raise ValueError('sampling gap exceeds one second')


def evaluate(directory, samples, tree, launch_unix_ms, expected):
    summary = json.loads((directory / 'summary.json').read_text())
    events = [json.loads(line) for line in (directory / 'events.ndjson').read_text().splitlines()]
    markers = {event['event']: event for event in events}
    clock = markers['scenario_clock']
    offset = clock['unix_ms'] - clock['elapsed_ms'] - launch_unix_ms
    idle = markers['settled_idle']['elapsed_ms'] + offset
    quiet = [row for row in samples if idle + 5000 <= row['elapsed_ms'] < idle + 15000]
    state = background_state(directory / 'library')
    validate_run(summary, events, quiet, state, expected)
    def since_launch(name):
        return markers[name]['elapsed_ms'] + offset if name in markers else None
    metrics = {
        'launch_to_first_frame_ms': since_launch('first_frame_painted'),
        'launch_to_first_visible_ms': since_launch('first_placement_visible'),
        'queue_to_all_visible_ms': (
            markers['all_placements_visible']['elapsed_ms'] - markers['scenario_started']['elapsed_ms']
            if summary['scenario'] != 'startup' else None),
        'launch_to_idle_ms': idle,
        'quiescent_cpu_percent': cpu_percent(quiet),
        'quiescent_rss_bytes': statistics.median(row['rss_bytes'] for row in quiet),
        'peak_tree_rss_bytes': sampling.tree_rss_stats(tree)['peak_tree_rss_bytes'],
    }
    for value in metrics.values():
        if value is not None and (not math.isfinite(value) or value < 0):
            raise ValueError(f'invalid metric: {metrics}')
    phases = sampling.phase_summary_from_events(events)
    if summary['scenario'] == 'startup':
        # Startup loading begins in the constructor, before scenario_started.
        phases['durations_ms'] = {}
    result = dict(valid=True, metrics=metrics, state=state, phases=phases,
                  per_format=sampling.per_format_groups_from_events(events),
                  helpers=sampling.per_helper_peaks(tree, main_pid=samples[0]['pid']),
                  quiescent_samples=len(quiet), clock_offset_ms=offset)
    write_json(directory / 'result.json', result)
    return result


def run_case(binary, assets, directory, data_root, scenario, timeout, expected=None):
    directory.mkdir(parents=True, exist_ok=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith('SNAPPYREF_')}
    env.update(SNAPPYREF_LIBRARY_ROOT=str(directory / 'library'), SNAPPYREF_DATA_ROOT=str(data_root),
               SNAPPYREF_PERF_IDLE_HOLD_SECS='20',
               SNAPPYREF_UPDATE_MANIFEST_URL='http://127.0.0.1:9/perf-no-updates')
    cmd = [str(binary), f'--perf-scenario={scenario}', f'--perf-corpus={assets}',
           f'--perf-run-dir={directory}', '--perf-log']
    samples, tree = [], []
    write_json(directory / 'launch.json', dict(command=cmd, environment={
        key: value for key, value in env.items() if key.startswith('SNAPPYREF_')}))
    with (directory / 'app.log').open('w') as log:
        launch_unix_ms = time.time() * 1000
        start = time.monotonic()
        process = subprocess.Popen(cmd, env=env, stdout=log, stderr=log, start_new_session=True)
        try:
            while process.poll() is None:
                summary_path = directory / 'summary.json'
                if summary_path.exists():
                    # macOS can defer a background window's scheduled close.
                    # End externally after a complete, shutdown-free window.
                    try:
                        settled = json.loads(summary_path.read_text())
                        events = sampling.load_events_ndjson(directory / 'events.ndjson')
                        clock = events[0]
                        idle_unix = clock['unix_ms'] - clock['elapsed_ms'] + settled['time_to_idle_ms']
                        if (time.monotonic() - start) * 1000 >= idle_unix - launch_unix_ms + 17000:
                            break
                    except json.JSONDecodeError:
                        pass
                if time.monotonic() - start > timeout:
                    raise TimeoutError(f'{scenario} exceeded {timeout}s')
                main = subprocess.run(['ps', '-p', str(process.pid), '-o', 'pid=,time=,rss='],
                                      capture_output=True, text=True, check=False).stdout.split()
                elapsed = (time.monotonic() - start) * 1000
                if len(main) == 3:
                    samples.append(dict(elapsed_ms=elapsed, pid=int(main[0]),
                                        cpu_time_seconds=cpu_seconds(main[1]), rss_bytes=int(main[2])*1024))
                probe = output('ps', '-axwwo', 'pid=,ppid=,rss=,%cpu=,command=')
                tick = (time.monotonic() - start) * 1000
                for row in sampling.select_descendants(sampling.parse_ps_axwo_output(probe), process.pid):
                    row.update(elapsed_ms=tick, role=sampling.classify_helper(row['command'], process.pid, row['pid']))
                    tree.append(row)
                time.sleep(0.2)
            if process.poll() is not None and process.returncode != 0:
                raise ValueError(f'app exited with status {process.returncode}')
            return evaluate(directory, samples, tree, launch_unix_ms, expected)
        except Exception as error:
            write_json(directory / 'result.json', dict(valid=False, error=str(error)))
            raise
        finally:
            # Kill only the private process group, including helpers after a timeout.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            for name, rows in [('samples.csv', samples), ('samples-tree.csv', tree)]:
                if rows:
                    with (directory / name).open('w') as handle:
                        writer = csv.DictWriter(handle, fieldnames=list(rows[0]))
                        writer.writeheader()
                        writer.writerows(rows)


def fixture_size(case):
    parts = case.split('-')
    if parts[0] == 'board':
        return 1, int(parts[1])
    if parts[0] == 'library':
        return int(parts[1]), int(parts[3]) if 'board' in parts else 0
    return None


def aggregate(runs):
    by_case = {}
    for run in runs:
        by_case.setdefault(run['case'], []).append(run)
    result = {}
    for case, trials in by_case.items():
        if not all(trial['valid'] for trial in trials):
            result[case] = dict(valid=False)
            continue
        metrics = {}
        for key in BUDGETS:
            values = [trial['metrics'][key] for trial in trials]
            if all(value is None for value in values):
                continue
            if any(value is None for value in values):
                raise ValueError(f'inconsistent metric {case}/{key}')
            metrics[key] = dict(median=statistics.median(values), min=min(values), max=max(values))
        result[case] = dict(valid=True, repeats=len(trials), metrics=metrics)
    return result


def compare(baseline, candidate):
    for report in (baseline, candidate):
        if report.get('schema') != 1 or not report.get('valid') or not report.get('cases'):
            raise ValueError('cannot compare invalid or empty evaluation')
        if any(not case['valid'] or case['repeats'] < 3 for case in report['cases'].values()):
            raise ValueError('comparison requires at least three valid repetitions per case')
    if baseline['contract'] != candidate['contract']:
        raise ValueError('machine, corpus, model, or measurement settings differ')
    if baseline['cases'].keys() != candidate['cases'].keys():
        raise ValueError('case lists differ')
    changes = []
    for case, before in baseline['cases'].items():
        after = candidate['cases'][case]
        if before['metrics'].keys() != after['metrics'].keys():
            raise ValueError(f'metric coverage differs for {case}')
        for metric, values in before['metrics'].items():
            old, new = values['median'], after['metrics'][metric]['median']
            if not all(math.isfinite(value) and value >= 0 for value in (old, new)):
                raise ValueError('nonfinite or negative comparison metric')
            relative, absolute = BUDGETS[metric]
            limit = max(old * relative, absolute)
            changes.append(dict(case=case, metric=metric, before=old, after=new,
                                allowance=limit, regression=new-old > limit))
    return dict(passed=not any(change['regression'] for change in changes), changes=changes)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    run = commands.add_parser('run')
    run.add_argument('cases', nargs='*', help='Scenario names, board-N or library-N[-board-N|-backfill]')
    run.add_argument('--suite', choices=SUITES)
    run.add_argument('--repeats', type=int, default=3)
    run.add_argument('--output', type=Path)
    run.add_argument('--timeout', type=float, default=180)
    run.add_argument('--binary', type=Path, help='Use this release binary without building')
    run.add_argument('--models', type=Path, default=ROOT / 'target/release/models')
    run.add_argument('--assets', type=Path, default=ROOT / 'perf-assets/images')
    diff = commands.add_parser('compare')
    diff.add_argument('baseline', type=Path)
    diff.add_argument('candidate', type=Path)
    diff.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.command == 'compare':
        result = compare(json.loads(args.baseline.read_text()), json.loads(args.candidate.read_text()))
        if args.output:
            write_json(args.output, result)
        failures = [change for change in result['changes'] if change['regression']]
        print(json.dumps(dict(passed=result['passed'], regressions=failures), indent=2))
        return 0 if result['passed'] else 1
    if sys.platform != 'darwin':
        raise ValueError('this sampler currently supports macOS only')
    # One local sampler at a time; overlapping runs contaminate CPU/RSS timings.
    (ROOT / 'target').mkdir(exist_ok=True)
    lock = (ROOT / 'target/perf-eval.lock').open('w')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise ValueError('another performance evaluation is running') from None
    cases = args.cases or SUITES[args.suite or 'core']
    if len(set(cases)) != len(cases) or args.repeats < 1 or args.timeout < 25:
        raise ValueError('unique cases, positive repeats, and timeout >=25 seconds required')
    binary = (args.binary or ROOT / 'target/release/snappyref').resolve()
    if args.binary is None:
        subprocess.run(['cargo', 'build', '--locked', '--release'], cwd=ROOT, check=True)
    directory = (args.output or ROOT / 'perf-assets/runs' / time.strftime('%Y%m%d-%H%M%S-eval')).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    assets = args.assets.resolve()
    manifest = json.loads((assets.parent / 'manifest.json').read_text())
    corpus_files = [('manifest.json', assets.parent / 'manifest.json')]
    corpus_files += [(entry['file'], assets / entry['file']) for entry in manifest]
    model_files = [(str(path.relative_to(args.models)), path) for path in args.models.rglob('*') if path.is_file()]
    if not model_files:
        raise ValueError('no installed models; pass --models with the installed model directory')
    contract = dict(platform=platform.platform(), cpu=output('sysctl', '-n', 'machdep.cpu.brand_string'),
                    memory_bytes=output('sysctl', '-n', 'hw.memsize'),
                    corpus_sha256=fingerprint(corpus_files), model_sha256=fingerprint(model_files),
                    evaluator_sha256=fingerprint([(name, ROOT / 'scripts' / name) for name in
                        ['perf_eval.py', '_perf_sampling.py', '_perf_library_seed.py']]),
                    power_source=output('pmset', '-g', 'batt').splitlines()[0],
                    sampling_interval_ms=200, idle_hold_seconds=20, external_stop_after_idle_ms=17000, quiescent_window_ms=[5000, 15000],
                    build='release/default-features', cache='fresh process; OS filesystem cache uncontrolled',
                    helper_policy='shipping defaults', fixture_version=1)
    data = directory / 'data'
    import shutil
    shutil.copytree(args.models, data / 'models', copy_function=copy_file)
    report = dict(schema=1, valid=False, contract=contract,
                  source=dict(commit=output('git', 'rev-parse', 'HEAD'),
                              dirty=bool(output('git', 'status', '--porcelain')),
                              diff_sha256=hashlib.sha256(output('git', 'diff', 'HEAD').encode()).hexdigest(),
                              binary_sha256=fingerprint([('snappyref', binary)]), rustc=output('rustc', '--version')),
                  runs=[], cases={})
    report_path = directory / 'evaluation.json'
    write_json(report_path, report)
    print(f'run_dir={directory}', flush=True)
    seed = directory / 'seed'
    if any(fixture_size(case) for case in cases):
        print('Preparing seed with real import and embedding', flush=True)
        run_case(binary, assets, seed, data, 'import-1', args.timeout)
    for repeat in range(1, args.repeats + 1):
        for case in cases:
            trial = directory / f'{case}-r{repeat}'
            size = fixture_size(case)
            print(f'[{repeat}/{args.repeats}] {case}', flush=True)
            try:
                if size:
                    trial.mkdir()
                    initial = seed_library(seed / 'library', trial / 'library', *size, ready=not case.endswith('-backfill'))
                    write_json(trial / 'fixture.json', initial)
                result = run_case(binary, assets, trial, data, 'startup' if size else case, args.timeout, size)
                report['runs'].append(dict(case=case, repeat=repeat, artifact=str(trial), **result))
                print(json.dumps(result['metrics']), flush=True)
            except Exception as error:
                report['runs'].append(dict(case=case, repeat=repeat, valid=False, error=str(error), artifact=str(trial)))
                write_json(report_path, report)
                raise
            report['cases'] = aggregate(report['runs'])
            write_json(report_path, report)
    report['valid'] = True
    write_json(report_path, report)
    print(f'evaluation={report_path}', flush=True)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, TimeoutError, subprocess.SubprocessError, sqlite3.Error, KeyError) as error:
        print(f'invalid evaluation: {error}', file=sys.stderr)
        sys.exit(2)
