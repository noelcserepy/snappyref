"""Offline issue-reporting outcomes through a stateful gh adapter."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('report-windows-ci-failure.sh')

STUB_GH = r'''#!/usr/bin/env python3
import json, os, subprocess, sys
state_path = os.environ['ISSUE_STATE']
state = json.load(open(state_path))
args = sys.argv[1:]
assert args[0] == 'issue', args
command, rest = args[1], args[2:]

def option(name, default=None):
    return rest[rest.index(name) + 1] if name in rest else default

if command == 'list':
    issues = [issue for issue in state['issues'] if issue['state'] == option('--state', 'open')]
    fields = option('--json', 'number,title').split(',')
    view = [{key: issue[key] for key in fields} for issue in issues]
    if '--jq' in rest:
        out = subprocess.run(['jq', '-r', option('--jq')], input=json.dumps(view),
                             capture_output=True, text=True, check=True).stdout
        sys.stdout.write(out)
    else:
        print(json.dumps(view))
elif command == 'create':
    number = max((issue['number'] for issue in state['issues']), default=0) + 1
    state['issues'].append(dict(number=number, state='open', title=option('--title'), body=option('--body')))
elif command == 'comment':
    state['comments'].append(dict(issue=int(rest[0]), body=option('--body')))
else:
    raise ValueError(command)
json.dump(state, open(state_path, 'w'))
'''


@unittest.skipUnless(shutil.which('jq'), 'jq is needed to evaluate gh --jq')
class WindowsCiFailureTests(unittest.TestCase):
    def test_creates_report_or_comments_on_existing_exact_title(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stub = root / 'gh'
            stub.write_text(STUB_GH)
            stub.chmod(0o755)
            title = 'Windows CI is red on main'
            body = 'Run: https://github.com/fixture/repo/actions/runs/42\nCommit: abc123'
            unrelated = [
                {'number': 11, 'state': 'open', 'title': 'Windows CI is red on main follow-up'},
                {'number': 12, 'state': 'closed', 'title': 'Windows CI is red on main'},
            ]
            for present in (False, True):
                with self.subTest(present=present):
                    issues = [*unrelated, *([{'number': 17, 'state': 'open', 'title': title}] if present else [])]
                    state = root / 'state.json'
                    state.write_text(json.dumps({'issues': issues, 'comments': []}))
                    env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', ISSUE_STATE=str(state),
                               GITHUB_SERVER_URL='https://github.com', GITHUB_REPOSITORY='fixture/repo',
                               GITHUB_RUN_ID='42', GITHUB_SHA='abc123')
                    subprocess.run(['bash', str(SCRIPT)], env=env, check=True)
                    expected = ({'issues': issues, 'comments': [{'issue': 17, 'body': body}]} if present else
                                {'issues': [*unrelated, {'number': 13, 'state': 'open', 'title': title, 'body': body}],
                                 'comments': []})
                    self.assertEqual(json.loads(state.read_text()), expected)


if __name__ == '__main__':
    unittest.main()
