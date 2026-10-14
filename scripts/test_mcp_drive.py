"""MCP drive deadlines and reports against disposable subprocesses."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('mcp_drive', ROOT / 'docs/skills/inspect/scripts/mcp-drive.py')
drive = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(drive)


class McpDriveTests(unittest.TestCase):
    def test_board_and_settings_replays_accept_app_values_and_reject_wrong_replies(self):
        cases = [
            ('create_board', {'name': 'Test'}, {'board': {'id': 7, 'name': 'Test'}}, True),
            ('create_board', {'name': None}, {'board': {'id': 8, 'name': 'Board 8'}}, True),
            ('create_board', None, {'board': {'id': 8, 'name': 'Board 8'}}, True),
            ('create_board', {'name': '\u00a0Test\x1c\u3000'},
             {'board': {'id': 7, 'name': 'Test\x1c'}}, True),
            ('rename_board', {'board': 7, 'name': '\u00a0Test\x1c\u3000'},
             {'board': {'id': 7, 'name': 'Test\x1c'}}, True),
            ('rename_board', {'board': 1, 'name': 'Test'},
             {'board': {'id': True, 'name': 'Test'}}, False),
            ('delete_board', {'board': 1}, {'deleted': 1, 'open': 2}, True),
            ('delete_board', {'board': 1}, {'deleted': True}, False),
            ('set_settings', {'placement_gap': 0.1, 'rotation_snap': 0.1, 'pointer': 'trackpad',
                              'sidebar_open': True, 'always_on_top': False},
             {'placement_gap': 0.10000000149011612, 'rotation_snap': 0.10000000149011612,
              'pointer': 'trackpad', 'sidebar_open': True, 'always_on_top': False}, True),
            ('set_settings', {'placement_gap': 999, 'rotation_snap': -1},
             {'placement_gap': 128.0, 'rotation_snap': 0.0}, True),
            ('set_settings', {'placement_gap': None, 'rotation_snap': None, 'pointer': None,
                              'sidebar_open': None, 'always_on_top': None},
             {'placement_gap': 42.0, 'rotation_snap': 15.0, 'pointer': 'mouse',
              'sidebar_open': False, 'always_on_top': True}, True),
            ('set_settings', None, {'placement_gap': 42.0, 'rotation_snap': 15.0,
                                   'pointer': 'mouse', 'sidebar_open': False, 'always_on_top': True}, True),
            ('set_settings', {'rotation_snap': 15}, {}, False),
            ('set_settings', {'pointer': 'trackpad'}, {'pointer': 'mouse'}, False),
            ('set_settings', {'placement_gap': 0.1}, {'placement_gap': 0.1}, False),
        ]
        for tool, arguments, reply, passed in cases:
            with self.subTest(tool=tool, arguments=arguments), tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                exe = root / 'fake-mcp'
                (root / 'reply.json').write_text(json.dumps(reply))
                exe.write_text(f'#!{sys.executable}\n' + '''import json, pathlib, sys
root = pathlib.Path(__file__).parent
for line in sys.stdin:
    message = json.loads(line)
    result = {'tools': []} if message['method'] == 'tools/list' else {}
    if message['method'] == 'tools/call':
        (root / 'request.json').write_text(json.dumps(message['params']))
        result = {'isError': False, 'content': [{'type': 'text', 'text': (root / 'reply.json').read_text()}]}
    print(json.dumps(dict(jsonrpc='2.0', id=message['id'], result=result)), flush=True)
''')
                exe.chmod(0o755)
                calls = root / 'calls.json'
                calls.write_text(json.dumps([[tool, arguments]]))
                with patch.object(drive, 'ROOT', root), \
                        patch.object(drive, 'grant_full_access', side_effect=lambda exe, env, library: library.mkdir(parents=True)), \
                        patch('sys.argv', ['mcp-drive', str(calls), '--exe', str(exe), '--timeout', '30']):
                    self.assertEqual(drive.main(), 0 if passed else 1)
                self.assertEqual(json.loads((root / 'request.json').read_text()),
                                 {'name': tool, 'arguments': arguments})
                artifact = Path((root / 'target/snappyref-inspect/mcp-drive/latest.txt').read_text())
                report = json.loads((artifact / 'report.json').read_text())
                self.assertEqual(report['status'], 'pass' if passed else 'fail')
                self.assertEqual(report['frames'], [])
                self.assertEqual(report['assertions'], [
                    {'type': 'assertion', 'passed': passed, 'name': f'01 {tool} answers'},
                ])
                summary = subprocess.run([sys.executable, str(ROOT / 'docs/skills/inspect/scripts/summarize-artifact.py'),
                                          str(artifact)], capture_output=True, text=True)
                self.assertEqual(summary.returncode, 0 if passed else 1, summary.stderr)

    def test_silent_and_partial_replies_timeout_and_reap_the_process(self):
        with tempfile.TemporaryDirectory() as scratch:
            exe = Path(scratch) / 'fake-mcp'
            for partial in ('', "os.write(1, b'{')"):
                with self.subTest(partial=partial):
                    exe.write_text(f'#!{sys.executable}\nimport os, signal, time\nsignal.signal(signal.SIGTERM, signal.SIG_IGN)\n{partial}\ntime.sleep(60)\n')
                    exe.chmod(0o755)
                    session = drive.McpSession(str(exe), os.environ, timeout=0.2)
                    started = time.monotonic()
                    try:
                        with self.assertRaisesRegex(TimeoutError, 'during initialize'):
                            session.request('initialize')
                    finally:
                        session.close()
                    self.assertLess(time.monotonic() - started, 5)
                    self.assertEqual(session.proc.returncode, -9)
                    with self.assertRaises(ProcessLookupError):
                        os.kill(session.proc.pid, 0)

    def test_failed_drive_writes_report_even_when_cleanup_fails(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            exe = root / 'fake-mcp'
            exe.write_text(f'#!{sys.executable}\n' + '''import json, sys
for line in sys.stdin:
    message = json.loads(line)
    result = {'tools': []} if message['method'] == 'tools/list' else {}
    if message['method'] == 'tools/call':
        result = {'isError': False, 'content': [{'type': 'text', 'text': '{"board":{"id":7,"name":"Wrong"}}'}]}
    print(json.dumps(dict(jsonrpc='2.0', id=message['id'], result=result)), flush=True)
''')
            exe.chmod(0o755)
            calls = root / 'calls.json'
            calls.write_text(json.dumps([['rename_board', {'board': 7, 'name': 'Renamed'}]]))
            processes = []
            original = drive.McpSession

            def session(*args):
                value = original(*args)
                processes.append(value.proc)
                return value

            def grant(exe, env, library):
                library.mkdir(parents=True)

            try:
                with patch.object(drive, 'ROOT', root), patch.object(drive, 'grant_full_access', grant), \
                        patch.object(drive, 'McpSession', side_effect=session), \
                        patch.object(original, 'app_pid', side_effect=RuntimeError('cleanup probe failed')), \
                        patch('sys.argv', ['mcp-drive', str(calls), '--exe', str(exe)]):
                    self.assertEqual(drive.main(), 1)
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.kill()
                        process.wait()
            artifact = Path((root / 'target/snappyref-inspect/mcp-drive/latest.txt').read_text())
            report = json.loads((artifact / 'report.json').read_text())
            self.assertEqual(report['status'], 'fail')
            self.assertEqual(report['assertions'], [
                {'type': 'assertion', 'passed': False, 'name': '01 rename_board answers'},
                {'type': 'assertion', 'passed': False, 'name': 'cleanup finishes: cleanup probe failed'},
            ])
            self.assertEqual(processes[0].returncode, 0)
            with self.assertRaises(ProcessLookupError):
                os.kill(processes[0].pid, 0)

    def test_reply_protocol_and_deterministic_result_checks(self):
        with tempfile.TemporaryDirectory() as scratch:
            exe = Path(scratch) / 'fake-mcp'
            exe.write_text(f'#!{sys.executable}\n' + '''import json, sys
for line in sys.stdin:
    message = json.loads(line)
    assert message == {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/call', 'params': {'name': 'rename_board', 'arguments': {'board': 7, 'name': 'Renamed'}}}
    print(json.dumps(dict(jsonrpc='2.0', id=1, result={'board': {'id': 7, 'name': 'Renamed'}})), flush=True)
''')
            exe.chmod(0o755)
            session = drive.McpSession(str(exe), os.environ)
            try:
                reply = session.request('tools/call', {'name': 'rename_board', 'arguments': {'board': 7, 'name': 'Renamed'}})
                self.assertEqual(reply, {'board': {'id': 7, 'name': 'Renamed'}})
                arguments = {'board': 7, 'name': ' Renamed '}
                self.assertTrue(drive.expected_result('rename_board', arguments, reply))
                self.assertFalse(drive.expected_result('rename_board', arguments, {'board': {'id': 7, 'name': 'Wrong'}}))
            finally:
                session.close()
            self.assertEqual(session.proc.returncode, 0)


if __name__ == '__main__':
    unittest.main()
