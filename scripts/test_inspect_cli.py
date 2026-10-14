"""Offline CLI checks exercise the real runner, artifact reader, and summarizer."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from test_inspect_scenarios import png_bytes

ROOT = Path(__file__).resolve().parents[1]


class InspectCliTests(unittest.TestCase):
    def test_runner_requires_fresh_valid_evidence_and_preserves_cargo_arguments(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch).resolve()
            runner = root / 'docs/skills/inspect/scripts/run-scenario.sh'
            runner.parent.mkdir(parents=True)
            shutil.copy2(ROOT / 'docs/skills/inspect/scripts/run-scenario.sh', runner)
            scripts = root / 'scripts'
            scripts.mkdir()
            for name in ('inspect_scenarios.py', 'inspect_artifact.py'):
                shutil.copy2(ROOT / 'scripts' / name, scripts / name)
            (root / 'pixel.png').write_bytes(png_bytes())
            cargo = root / 'cargo'
            cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
root = pathlib.Path.cwd()
(root / 'arguments.json').write_text(json.dumps(sys.argv[1:]))
mode = os.environ['EVIDENCE_MODE']
if mode == 'missing':
    sys.exit(0)
scenario = os.environ['ARTIFACT_SCENARIO']
target = pathlib.Path(os.environ.get('CARGO_TARGET_DIR', 'target'))
if target.is_absolute():
    sys.exit(7)
folder = root / target / 'snappyref-inspect' / scenario
artifact = folder / 'run-1'
artifact.mkdir(parents=True, exist_ok=True)
frame = artifact / 'frame.png'
shutil.copyfile(root / 'pixel.png', frame)
report = dict(scenario=scenario, artifact_dir=str(artifact), status='pass',
              frames=[str(frame)], assertions=[dict(name='image visible', passed=True)])
if mode == 'wrong-scenario':
    report['scenario'] = 'other'
if mode == 'corrupt':
    frame.write_bytes(b'corrupt PNG')
(artifact / 'report.json').write_text(json.dumps(report))
(folder / 'latest.txt').write_text(str(artifact))
if mode == 'cargo-failure':
    sys.exit(7)
if mode == 'stale':
    for file in (artifact / 'report.json', folder / 'latest.txt'):
        os.utime(file, (1, 1))
''')
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', ARTIFACT_SCENARIO='sample-image-import')
            env.pop('CARGO_TARGET_DIR', None)
            for mode in ('fresh', 'missing', 'stale', 'cargo-failure', 'wrong-scenario', 'corrupt'):
                with self.subTest(mode=mode):
                    shutil.rmtree(root / 'target', ignore_errors=True)
                    result = subprocess.run(['bash', str(runner), 'sample-image-import'],
                                            env={**env, 'EVIDENCE_MODE': mode}, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0 if mode == 'fresh' else 1, result.stderr)
                    self.assertEqual(json.loads((root / 'arguments.json').read_text()),
                                     ['test', '--locked', '--lib', 'inspect_sample_image_import_writes_artifact', '--', '--nocapture'])
                    if mode == 'fresh':
                        artifact = root / 'target/snappyref-inspect/sample-image-import/run-1'
                        self.assertEqual(result.stdout, f'status=pass\nscenario=sample-image-import\nartifact={artifact}\n')
                        summary = subprocess.run(['python3', str(ROOT / 'docs/skills/inspect/scripts/summarize-artifact.py'), str(artifact)], capture_output=True, text=True)
                        self.assertEqual(summary.returncode, 0, summary.stderr)
                        self.assertIn('- pass: image visible\n', summary.stdout)
            result = subprocess.run(['bash', str(runner), 'check-harness'],
                                    env={**env, 'EVIDENCE_MODE': 'fresh', 'ARTIFACT_SCENARIO': 'search-drag-to-canvas'}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads((root / 'arguments.json').read_text()),
                             ['test', '--locked', '--lib', 'inspect_search_result_drag_to_canvas_writes_artifact', '--', '--nocapture'])
            for mode in ('fresh', 'corrupt'):
                result = subprocess.run(['bash', str(runner), 'sample-image-import'],
                                        env={**env, 'CARGO_TARGET_DIR': 'build', 'EVIDENCE_MODE': mode}, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0 if mode == 'fresh' else 1, result.stderr)
                self.assertIn(f'artifact={root / "build/snappyref-inspect/sample-image-import/run-1"}\n', result.stdout)
            (root / 'arguments.json').unlink()
            result = subprocess.run(['bash', str(runner), 'sample-image-import'],
                                    env={**env, 'CARGO_TARGET_DIR': str(root.parent), 'EVIDENCE_MODE': 'fresh'}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn('CARGO_TARGET_DIR points outside this checkout', result.stdout)
            self.assertFalse((root / 'arguments.json').exists())
            result = subprocess.run(['bash', str(runner), 'unknown'], env=env, capture_output=True)
            self.assertEqual(result.returncode, 2)

    def test_summarizer_rejects_empty_wrong_scenario_and_corrupt_frame_reports(self):
        with tempfile.TemporaryDirectory() as scratch:
            artifact = Path(scratch) / 'sample/run-1'
            artifact.mkdir(parents=True)
            frame = artifact / 'frame.png'
            report = dict(scenario='sample', artifact_dir=str(artifact), status='pass',
                          frames=[str(frame)], assertions=[dict(name='image visible', passed=True)])
            for changed, image in ((report, png_bytes()), ({}, png_bytes()),
                                   ({**report, 'scenario': 'other'}, png_bytes()),
                                   (report, png_bytes()[:-1]), (report, b'bad')):
                frame.write_bytes(image)
                (artifact / 'report.json').write_text(json.dumps(changed))
                result = subprocess.run(['python3', str(ROOT / 'docs/skills/inspect/scripts/summarize-artifact.py'), str(artifact)], capture_output=True)
                self.assertEqual(result.returncode, 0 if changed == report and image == png_bytes() else 1)


if __name__ == '__main__':
    unittest.main()
