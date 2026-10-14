"""Source synchronization inventory protects machine files and tracks removals."""
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest

source_paths = runpy.run_path(str(Path(__file__).with_name('windows-sync.py')))['source_paths']


def setUpModule():
    # Run git against the scratch repos only. A git hook or `rebase --exec`
    # exports GIT_DIR, and `git init` under it rewrites the outer repo's config.
    local_vars = subprocess.run(['git', 'rev-parse', '--local-env-vars'],
                                capture_output=True, text=True, check=True).stdout.split()
    for name in local_vars:
        os.environ.pop(name, None)


class WindowsSyncTests(unittest.TestCase):
    def test_source_inventory_follows_renames_and_deletions_without_machine_files(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / 'src').mkdir()
            for name in ('Cargo.toml', 'src/main.rs', 'old.rs', 'removed.rs'):
                (root / name).write_text('fixture')
            (root / '.gitignore').write_text('target/\n.env.windows\n')
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            (root / 'target').mkdir()
            (root / 'target/cache').write_text('private')
            (root / '.env.windows').write_text('private')
            (root / 'old.rs').rename(root / 'new.rs')
            (root / 'removed.rs').unlink()
            files = source_paths(root)
            self.assertIn('new.rs', files)
            self.assertNotIn('old.rs', files)
            self.assertNotIn('removed.rs', files)
            self.assertNotIn('.env.windows', files)
            self.assertNotIn('target/cache', files)
            subprocess.run(['git', 'add', '-A'], cwd=root, check=True)
            self.assertEqual(source_paths(root), files)
            (root / 'bad:name').write_text('unsafe')
            with self.assertRaises(ValueError):
                source_paths(root)
