"""Exercise the pre-push boundary with offline check commands."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1] / '.githooks/pre-push'


class PrePushTests(unittest.TestCase):
    def run_hook(self, *, fail='', deletion=False, code=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            if code:
                (root / 'Cargo.toml').touch()
            log = root / 'commands'
            for command in ('cargo', 'python3'):
                stub = root / command
                stub.write_text('#!/bin/sh\nprintf "%s\\n" "$(basename "$0") $*" >> "$CHECK_LOG"\n[ "$*" != "$FAIL_CHECK" ]\n')
                stub.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', CHECK_LOG=str(log), FAIL_CHECK=fail)
            sha = '0' * 40 if deletion else '1' * 40
            result = subprocess.run(['bash', str(HOOK)], cwd=root, env=env,
                                    input=f'refs/heads/topic {sha} refs/heads/topic {"2" * 40}\n',
                                    text=True, capture_output=True)
            return result.returncode, log.read_text().splitlines() if log.exists() else []

    def test_checks_run_in_order_and_stop_at_failure(self):
        commands = ['cargo fmt --check', 'cargo clippy --locked --all-targets -- -D warnings',
                    'cargo test --locked', "python3 -m unittest discover -s scripts -p test_*.py"]
        self.assertEqual(self.run_hook(), (0, commands))
        for index, command in enumerate(commands):
            with self.subTest(command=command):
                self.assertEqual(self.run_hook(fail=command.split(' ', 1)[1]),
                                 (1, commands[:index + 1]))
        self.assertEqual(self.run_hook(deletion=True), (0, []))
        self.assertEqual(self.run_hook(code=False), (0, []))


if __name__ == '__main__':
    unittest.main()
