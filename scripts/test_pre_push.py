"""Exercise the pre-push boundary with offline check commands."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOOK = Path(__file__).resolve().parents[1] / '.githooks/pre-push'


CHECKS = ['cargo fmt --check', 'cargo clippy --quiet --locked --all-targets -- -D warnings',
          'cargo test --quiet --locked', "python3 -m unittest discover --buffer -s scripts -p test_*.py",
          'tests/scripts/test-build-signed-manifest.sh']

# Each stub logs and prints the command as the hook names it, and fails when
# that is FAIL_CHECK. Tools resolve through PATH, the manifest test by path.
STUB = ('#!/bin/sh\nline="${0#"$STUB_ROOT"/}${*:+ $*}"\n'
        'printf "%s\\n" "$line${GIT_DIR+ GIT_DIR=$GIT_DIR}" >> "$CHECK_LOG"\n'
        'echo "$line output"\n[ "$line" != "$FAIL_CHECK" ]\n')


class PrePushTests(unittest.TestCase):
    def run_hook(self, *, fail='', deletion=False, code=True, git_env=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            if code:
                (root / 'Cargo.toml').touch()
            log = root / 'commands'
            for command in ('cargo', 'python3', 'tests/scripts/test-build-signed-manifest.sh'):
                stub = root / command
                stub.parent.mkdir(parents=True, exist_ok=True)
                stub.write_text(STUB)
                stub.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', STUB_ROOT=str(root), CHECK_LOG=str(log),
                       FAIL_CHECK=fail, **(git_env or {}))
            sha = '0' * 40 if deletion else '1' * 40
            result = subprocess.run(['bash', str(HOOK)], cwd=root, env=env,
                                    input=f'refs/heads/topic {sha} refs/heads/topic {"2" * 40}\n',
                                    text=True, capture_output=True)
            return result.returncode, log.read_text().splitlines() if log.exists() else [], result.stdout

    def test_checks_run_in_order_and_stop_at_failure(self):
        self.assertEqual(self.run_hook()[:2], (0, CHECKS))
        for index, command in enumerate(CHECKS):
            with self.subTest(command=command):
                self.assertEqual(self.run_hook(fail=command)[:2],
                                 (1, CHECKS[:index + 1]))
        self.assertEqual(self.run_hook(deletion=True)[:2], (0, []))
        self.assertEqual(self.run_hook(code=False)[:2], (0, []))

    def test_only_a_failing_check_prints_its_output(self):
        self.assertEqual(self.run_hook()[2], ''.join(f'[pre-push] {command}\n' for command in CHECKS)
                         + '[pre-push] ok\n')
        self.assertEqual(self.run_hook(fail='cargo clippy --quiet --locked --all-targets -- -D warnings')[2],
                         '[pre-push] cargo fmt --check\n'
                         '[pre-push] cargo clippy --quiet --locked --all-targets -- -D warnings\n'
                         'cargo clippy --quiet --locked --all-targets -- -D warnings output\n')

    def test_checks_run_without_the_pushing_repository_in_git_env(self):
        # Git exports GIT_DIR to hooks, so a test's `git init` would rewrite the pushing repo's config.
        self.assertEqual(self.run_hook(git_env={'GIT_DIR': '/repo/.git/worktrees/topic'})[:2], (0, CHECKS))


if __name__ == '__main__':
    unittest.main()
