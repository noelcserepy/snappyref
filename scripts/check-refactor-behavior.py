#!/usr/bin/env python3
"""Run behavior tests and verify fresh inspect artifacts before a refactor.

The library suite covers deterministic interactions/storage. The release helper
checks exercise the subprocess path that cfg(test) deliberately bypasses.
"""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import zlib

ROOT = Path(__file__).resolve().parent.parent
SCENARIOS = ['sample-image-import', 'pending-drop-batch-undo', 'search-drag-to-canvas',
             'blue-image-search-drag', 'board-lifecycle-undo-redo', 'canvas-edit-operations',
             'canvas-navigation-boards', 'session-restart-restore', 'persistence-dropped-write',
             'delete-undo-redo-placement', 'crop-undo-redo-placement']


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def check_imageprep(binary):
    with tempfile.TemporaryDirectory(prefix='snappyref-helper-check-') as scratch:
        directory = Path(scratch)
        source = directory / 'source.png'
        def chunk(kind, data):
            return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
        rgba = bytes([30, 100, 220, 255])
        source.write_bytes(b'\x89PNG\r\n\x1a\n' +
                          chunk(b'IHDR', struct.pack('>IIBBBBB', 64, 32, 8, 6, 0, 0, 0)) +
                          chunk(b'IDAT', zlib.compress((b'\0' + rgba * 64) * 32)) + chunk(b'IEND', b''))
        target = directory / 'output.rgba'
        result = subprocess.run([str(binary), '--imageprep', f'--source={source}',
                                 f'--rgba-output={target}', '--canvas-max-side=16'],
                                capture_output=True, text=True, timeout=30, check=True)
        decoded = json.loads(result.stdout)
        require((decoded['source_width'], decoded['source_height']) == (64, 32), 'source dimensions lost')
        require((decoded['width'], decoded['height']) == (16, 8), 'downscale dimensions changed')
        require(target.read_bytes() == rgba * 16 * 8, 'helper changed solid RGBA pixels')
        require(Path(decoded['cache_path']).is_file(), 'prepared cache missing')
        target.unlink()
        source.write_bytes(b'not an image')
        failed = subprocess.run([str(binary), '--imageprep', f'--source={source}',
                                 f'--rgba-output={target}', '--canvas-max-side=16'],
                                capture_output=True, text=True, timeout=30)
        require(failed.returncode != 0 and not target.exists(), 'corrupt image reported success')
    return ['release helper downscales and transfers exact pixels', 'release helper rejects corrupt image']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/refactor-behavior.json')
    args = parser.parse_args()
    subprocess.run(['cargo', 'check', '--locked'], cwd=ROOT, check=True)
    started = time.time()
    subprocess.run(['cargo', 'test', '--locked', '--lib', '--', '--test-threads=1'], cwd=ROOT, check=True)
    artifacts = []
    for scenario in SCENARIOS:
        latest = ROOT / 'target/snappyref-inspect' / scenario / 'latest.txt'
        require(latest.stat().st_mtime >= started, f'stale inspect artifact: {scenario}')
        artifact = Path(latest.read_text().strip())
        report = json.loads((artifact / 'report.json').read_text())
        require(report['status'] == 'pass' and report['assertions'] and report['frames'],
                f'incomplete inspect report: {scenario}')
        require(all(item['passed'] for item in report['assertions']), f'failed assertion: {scenario}')
        require(all(Path(frame).is_file() for frame in report['frames']), f'missing frames: {scenario}')
        artifacts.append(dict(scenario=scenario, artifact=str(artifact), assertions=len(report['assertions'])))
    subprocess.run(['cargo', 'build', '--locked', '--release'], cwd=ROOT, check=True)
    helpers = check_imageprep(ROOT / 'target/release/snappyref')
    result = dict(passed=True, inspect=artifacts, release_helper_checks=helpers,
                  limitations=['networked real model installer excluded',
                               'native clipboard, external drag/drop, and Windows require platform checks'])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(f'behavior_report={args.output}')


if __name__ == '__main__':
    main()
