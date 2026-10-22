"""Push through the real pre-push hook and check which commits it refuses."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PrivateValuesPushTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        base = Path(scratch.name)
        self.repo = base / 'checkout'
        (self.repo / 'scripts').mkdir(parents=True)
        shutil.copytree(ROOT / '.githooks', self.repo / '.githooks')
        shutil.copy(ROOT / 'scripts' / 'check-private-values.py', self.repo / 'scripts')
        env_file = base / 'env.windows'
        env_file.write_text("SNAPPYREF_WINDOWS_HOST='pc-hidden-alias'\n"
                            "SNAPPYREF_WINDOWS_REPO='C:\\Users\\fakeperson\\src\\snappyref'\n")
        home = base / 'home-of-tester'
        home.mkdir()
        self.env = dict(os.environ, HOME=str(home), GIT_CONFIG_GLOBAL=os.devnull,
                        SNAPPYREF_WINDOWS_ENV=str(env_file))
        subprocess.run(['git', 'init', '-q', '--bare', str(base / 'origin.git')], check=True)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.email', 'test@example.com')
        self.git('config', 'user.name', 'Test')
        self.git('config', 'core.hooksPath', '.githooks')
        self.git('remote', 'add', 'origin', str(base / 'origin.git'))
        self.commit('README.md', 'board\n', 'Start')
        self.git('push', '-q', 'origin', 'main')

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.repo, env=self.env, text=True,
                              capture_output=True, timeout=30)

    def commit(self, name, text, message):
        (self.repo / name).write_text(text)
        self.git('add', name)
        self.assertEqual(self.git('commit', '-q', '-m', message).returncode, 0)

    def test_refuses_a_windows_path_in_any_spelling_then_takes_the_cleaned_commit(self):
        self.commit('notes.md', 'Build there:\nC:/users/FakePerson/other\n', 'Add notes')

        push = self.git('push', 'origin', 'main')

        self.assertEqual(push.returncode, 1)
        self.assertIn('notes.md:2: SNAPPYREF_WINDOWS_REPO', push.stderr)
        self.assertNotIn('fakeperson', push.stderr.lower())
        (self.repo / 'notes.md').write_text('Build in the checkout.\n')
        self.assertEqual(self.git('commit', '-qa', '--amend', '--no-edit').returncode, 0)
        self.assertEqual(self.git('push', 'origin', 'main').returncode, 0)

    def test_refuses_the_host_and_home_path_in_a_message_and_a_new_branch(self):
        self.git('switch', '-q', '-c', 'pr-assets')
        self.commit('log.txt', f'saved to {self.env["HOME"]}/shots\n',
                    'Captured on pc-hidden-alias')

        push = self.git('push', 'origin', 'pr-assets')

        self.assertEqual(push.returncode, 1)
        self.assertIn('commit message: SNAPPYREF_WINDOWS_HOST', push.stderr)
        self.assertIn('log.txt:1: HOME', push.stderr)
        self.assertNotIn('pc-hidden-alias', push.stderr)

    def test_does_not_rescan_commits_the_remote_already_has(self):
        self.commit('old.md', 'pc-hidden-alias\n', 'Old leak')
        self.assertEqual(self.git('push', '--no-verify', 'origin', 'main').returncode, 0)
        self.commit('new.md', 'clean\n', 'Clean change')

        self.assertEqual(self.git('push', 'origin', 'main').returncode, 0)


if __name__ == '__main__':
    unittest.main()
