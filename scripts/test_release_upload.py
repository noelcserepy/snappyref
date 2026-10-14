"""Drive upload-release-assets.sh against a stub gh that keeps release state in a file."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/upload-release-assets.sh'

# Supports the gh calls the script makes. --jq runs through real jq, as gh does.
STUB_GH = r'''#!/usr/bin/env python3
import json, os, subprocess, sys
state_path = os.environ['STUB_GH_STATE']
state = json.load(open(state_path))
args = sys.argv[1:]
assert args[0] == 'release', args
command, tag, rest = args[1], args[2], args[3:]
release = state.get(tag)

def option(name):
    return rest[rest.index(name) + 1] if name in rest else None

if command == 'view':
    if release is None:
        sys.exit(1)
    if '--json' in rest:
        view = {'isDraft': release['draft'], 'assets': [{'name': n} for n in release['assets']]}
        out = subprocess.run(['jq', '-r', option('--jq')], input=json.dumps(view),
                             capture_output=True, text=True, check=True).stdout
        sys.stdout.write(out)
elif command == 'create':
    state[tag] = {'draft': '--draft' in rest, 'target': option('--target'), 'assets': {}}
elif command == 'edit':
    release['target'] = option('--target')
elif command == 'upload':
    for path in rest:
        if path != '--clobber':
            release['assets'][os.path.basename(path)] = open(path).read()
json.dump(state, open(state_path, 'w'))
'''


@unittest.skipUnless(shutil.which('jq'), 'jq is needed to evaluate gh --jq')
class ReleaseUploadTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.dir = Path(scratch.name)
        (self.dir / 'bin').mkdir()
        gh = self.dir / 'bin/gh'
        gh.write_text(STUB_GH)
        gh.chmod(0o755)
        self.state = self.dir / 'state.json'
        for name, content in [('snappyref-mac-v0.3.1.zip', 'new mac'), ('latest.json', 'new manifest')]:
            (self.dir / name).write_text(content)

    def upload(self, releases, target, *names):
        self.state.write_text(json.dumps(releases))
        env = dict(os.environ, PATH=f"{self.dir / 'bin'}:{os.environ['PATH']}", STUB_GH_STATE=str(self.state))
        result = subprocess.run([SCRIPT, 'v0.3.1', target, *[str(self.dir / n) for n in names]],
                                env=env, capture_output=True, text=True)
        return result, json.loads(self.state.read_text()).get('v0.3.1')

    def signed_draft(self, draft=True):
        return {'v0.3.1': {'draft': draft, 'target': 'old', 'assets': {
            'snappyref-mac-v0.3.1.zip': 'old mac', 'latest.json': 'old manifest'}}}

    def test_creates_a_draft_at_the_target_when_the_tag_has_no_release(self):
        result, release = self.upload({}, 'abc123', 'snappyref-mac-v0.3.1.zip')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(release, {'draft': True, 'target': 'abc123',
                                   'assets': {'snappyref-mac-v0.3.1.zip': 'new mac'}})

    def test_signed_draft_takes_new_zips_only_with_a_new_manifest(self):
        result, release = self.upload(self.signed_draft(), '', 'snappyref-mac-v0.3.1.zip')
        self.assertEqual(result.returncode, 1)
        self.assertIn('holds a signed latest.json', result.stderr)
        self.assertEqual(release, self.signed_draft()['v0.3.1'])

        result, release = self.upload(self.signed_draft(), 'abc123', 'snappyref-mac-v0.3.1.zip', 'latest.json')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(release, {'draft': True, 'target': 'abc123', 'assets': {
            'snappyref-mac-v0.3.1.zip': 'new mac', 'latest.json': 'new manifest'}})

    def test_published_release_is_never_rewritten(self):
        published = self.signed_draft(draft=False)
        result, release = self.upload(published, 'abc123', 'snappyref-mac-v0.3.1.zip', 'latest.json')
        self.assertEqual(result.returncode, 1)
        self.assertIn('already published', result.stderr)
        self.assertEqual(release, published['v0.3.1'])


if __name__ == '__main__':
    unittest.main()
