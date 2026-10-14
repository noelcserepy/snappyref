"""The manifest runner must reject each bad checksum or size on system Bash."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]

# Signing has its own real-key round-trip suite. This producer and crypto stub
# isolate the runner's assertion propagation from that independent behavior.
PRODUCER = r'''
import hashlib, json, os, pathlib, sys
options = dict(zip(sys.argv[1::2], sys.argv[2::2]))
if options['--public-key'].endswith('other-pub.pem'):
    sys.exit(1)
platforms = {}
for platform in ('macos', 'windows'):
    data = pathlib.Path(options[f'--{platform}-zip']).read_bytes()
    digest, size = hashlib.sha256(data).hexdigest(), len(data)
    if os.environ['CORRUPT_FIELD'] == f'{platform}.sha256':
        digest = '0' * 64
    if os.environ['CORRUPT_FIELD'] == f'{platform}.size':
        size += 1
    platforms[platform] = dict(url=options[f'--{platform}-url'], sha256=digest, size=size)
report = dict(channel='stable', version=options['--version'], released_at=options['--released-at'],
              notes_url=options['--notes-url'], platforms=platforms, signature='ab' * 64)
pathlib.Path(options['--output']).write_text(json.dumps(report))
'''


@unittest.skipUnless(Path('/bin/bash').is_file(), 'system Bash is required')
class SignedManifestRunnerTests(unittest.TestCase):
    def test_valid_manifest_passes_and_each_incorrect_hash_or_size_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runner = root / 'tests/scripts/test-build-signed-manifest.sh'
            runner.parent.mkdir(parents=True)
            shutil.copy2(ROOT / 'tests/scripts/test-build-signed-manifest.sh', runner)
            producer = root / 'scripts/build-signed-manifest.sh'
            producer.parent.mkdir()
            producer.write_text(f'#!{sys.executable}\n' + PRODUCER)
            producer.chmod(0o755)
            (root / 'bin').mkdir()
            crypto = root / 'bin/openssl'
            crypto.write_text('#!/bin/sh\nexit 0\n')
            crypto.chmod(0o755)
            for field in ('', 'macos.sha256', 'windows.sha256', 'macos.size', 'windows.size'):
                with self.subTest(field=field):
                    env = dict(os.environ, PATH=f'{root / "bin"}:{os.environ["PATH"]}', CORRUPT_FIELD=field)
                    result = subprocess.run(['/bin/bash', str(runner)], env=env, text=True,
                                            capture_output=True, timeout=60)
                    self.assertEqual(result.returncode, 1 if field else 0, result.stderr)
                    self.assertEqual(result.stdout.splitlines()[-2:],
                                     ['passed: 3', 'failed: 1'] if field else ['passed: 4', 'failed: 0'])


if __name__ == '__main__':
    unittest.main()
