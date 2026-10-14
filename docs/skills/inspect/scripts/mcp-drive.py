#!/usr/bin/env python3
"""Drives the real app through `snappyref --mcp` on a throwaway data root.

Replays a JSON list of [tool, arguments] calls. A third element, "refused",
marks a call the app must refuse. `$REPO` and `$OUT` in the calls file expand to
this checkout and the artifact directory. An argument of `"$3.board.id"` takes
that value from the reply to call 3, so ids come from the app. The artifact has
the shape a scenario
writes: report.json with one assertion per call, events.ndjson with each call's
arguments and reply, and frames/ with every returned image.

`--mcp` starts the app itself, behind the frontmost window, and it never takes
focus. `screenshot` answers from there too.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[4]


def wait_for(what, check, seconds=60):
    deadline = time.time() + seconds
    while time.time() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise RuntimeError(f'timed out waiting for {what}')


def session_row(db_path):
    if not db_path.is_file():
        return None
    # A read-only open fails while the app holds the WAL, so open it plainly.
    db = sqlite3.connect(db_path, timeout=1)
    try:
        row = db.execute('select session_payload from user_session where id = 1').fetchone()
    except sqlite3.Error:
        return None
    finally:
        db.close()
    return row and row[0]


def grant_full_access(exe, env, library):
    """Access lives in the session row, which only a first launch creates."""
    db_path = library / 'library.db'
    first = subprocess.Popen([exe, '--no-startup-focus'], env=env, stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        wait_for('the first launch to write its session', lambda: session_row(db_path))
    finally:
        first.terminate()
        try:
            first.wait(timeout=20)
        except subprocess.TimeoutExpired:
            first.kill()
            first.wait()
    payload = json.loads(session_row(db_path))
    settings = payload['workspace_settings']
    settings['ai_control'] = 'full'
    # The first-launch prompt would cover the board in every screenshot.
    settings['semantic_search_first_launch_prompt_seen'] = True
    db = sqlite3.connect(db_path)
    with db:
        db.execute('update user_session set session_payload = ? where id = 1', (json.dumps(payload),))
    db.close()


def from_replies(value, replies):
    """Swaps each "$N.path" string for that value in the reply to call N."""
    if isinstance(value, dict):
        return {key: from_replies(item, replies) for key, item in value.items()}
    if isinstance(value, list):
        return [from_replies(item, replies) for item in value]
    ref = isinstance(value, str) and re.fullmatch(r'\$(\d+)\.(.+)', value)
    if not ref:
        return value
    try:
        found = replies[int(ref[1])]
        for key in ref[2].split('.'):
            found = found[int(key)] if isinstance(found, list) else found[key]
    except (LookupError, TypeError, ValueError):
        raise LookupError(f'{value} is not in the reply to call {ref[1]}') from None
    return found


class McpSession:
    def __init__(self, exe, env, timeout=60):
        self.timeout = timeout
        self.buffer = b''
        self.proc = subprocess.Popen([exe, '--mcp'], env=env,
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        self.next_id = 0

    def request(self, method, params=None):
        self.next_id += 1
        message = {'jsonrpc': '2.0', 'id': self.next_id, 'method': method}
        if params is not None:
            message['params'] = params
        self.proc.stdin.write((json.dumps(message) + '\n').encode())
        self.proc.stdin.flush()
        deadline = time.monotonic() + self.timeout
        with selectors.DefaultSelector() as selector:
            selector.register(self.proc.stdout, selectors.EVENT_READ)
            while b'\n' not in self.buffer:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not selector.select(remaining):
                    raise TimeoutError(f'timed out waiting for `--mcp` during {method}')
                chunk = os.read(self.proc.stdout.fileno(), 65536)
                if not chunk:
                    raise RuntimeError(f'`--mcp` closed stdout during {method}')
                self.buffer += chunk
        line, self.buffer = self.buffer.split(b'\n', 1)
        response = json.loads(line)
        if response.get('id') != self.next_id or 'result' not in response:
            raise RuntimeError(f'invalid `--mcp` response during {method}: {response}')
        return response['result']

    def app_pid(self):
        """The GUI that `--mcp` started is its only child."""
        found = subprocess.run(['pgrep', '-P', str(self.proc.pid)], capture_output=True, text=True)
        pids = found.stdout.split()
        return int(pids[0]) if pids else None

    def close(self):
        try:
            self.proc.stdin.close()
        finally:
            self.reap()

    def reap(self):
        try:
            self.proc.wait(timeout=self.timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        finally:
            self.proc.stdout.close()


def trim_board_name(name):
    # Match Rust's whitespace set, preserving Python-only U+001C..U+001F.
    return name.strip('\t\n\v\f\r \u0085\u00a0\u1680\u2000\u2001\u2002\u2003\u2004\u2005'
                      '\u2006\u2007\u2008\u2009\u200a\u2028\u2029\u202f\u205f\u3000')


def expected_result(tool, arguments, reply):
    """Check deterministic fields without pinning generated IDs or viewport state."""
    # The app's argument boundary turns JSON null into an empty object.
    if arguments is None:
        arguments = {}
    if tool == 'create_board' and arguments.get('name') is not None:
        return isinstance(reply, dict) and reply.get('board', {}).get('name') == trim_board_name(arguments['name'])
    if tool == 'rename_board':
        board = reply.get('board') if isinstance(reply, dict) else None
        return (isinstance(board, dict) and type(board.get('id')) is int
                and reply == {'board': {'id': arguments['board'], 'name': trim_board_name(arguments['name'])}})
    if tool == 'delete_board':
        return (isinstance(reply, dict) and type(reply.get('deleted')) is int
                and reply['deleted'] == arguments['board'])
    if tool == 'set_settings':
        if not isinstance(reply, dict):
            return False
        for key, value in arguments.items():
            # Optional null settings leave the current value unchanged.
            if value is None:
                continue
            actual = reply.get(key)
            if key in ('placement_gap', 'rotation_snap'):
                limit = 128 if key == 'placement_gap' else 180
                # The app clamps these settings and serializes their f32 value.
                accepted = struct.unpack('!f', struct.pack('!f', min(max(value, 0), limit)))[0]
                if type(actual) not in (int, float) or actual != accepted:
                    return False
            elif key in ('sidebar_open', 'always_on_top', 'pointer'):
                if type(actual) is not type(value) or actual != value:
                    return False
            else:
                return False
        return True
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('calls', type=Path, help='JSON list of [tool, arguments] or [tool, arguments, "refused"]')
    parser.add_argument('--exe', type=Path, default=ROOT / 'target/debug/snappyref')
    parser.add_argument('--timeout', type=float, default=60, help='deadline in seconds for each MCP reply and shutdown')
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    if not args.exe.is_file():
        parser.error(f'{args.exe} is missing. Run `cargo build --locked --bin snappyref` first.')

    scenario_dir = ROOT / 'target/snappyref-inspect/mcp-drive'
    artifact = scenario_dir / f'run-{time.time_ns()}'
    (artifact / 'frames').mkdir(parents=True)
    calls = json.loads(args.calls.read_text().replace('$REPO', str(ROOT)).replace('$OUT', str(artifact)))
    data_root = Path(tempfile.mkdtemp(prefix='snappyref-mcp-'))
    # The update check would otherwise reach the published release.
    env = dict(os.environ, SNAPPYREF_DATA_ROOT=str(data_root),
               SNAPPYREF_UPDATE_MANIFEST_URL='http://127.0.0.1:9/mcp-drive-no-updates')
    env.pop('SNAPPYREF_LIBRARY_ROOT', None)
    endpoint = data_root / 'library/control.json'
    session, app_pid = None, None
    tools, assertions, frames, replies = [], [], [], {}
    try:
        grant_full_access(str(args.exe), env, data_root / 'library')
        # An endpoint nothing answers on makes `--mcp` start the app itself.
        endpoint.write_text(json.dumps({'port': 1, 'token': 'stale'}))
        session = McpSession(str(args.exe), env, args.timeout)
        session.request('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                       'clientInfo': {'name': 'mcp-drive', 'version': '0'}})
        tools = [tool['name'] for tool in session.request('tools/list')['tools']]
        with open(artifact / 'events.ndjson', 'w') as events:
            events.write(json.dumps({'type': 'observation.fact',
                                     'payload': {'name': 'tools', 'value': tools}}) + '\n')
            for index, (tool, arguments, *expect) in enumerate(calls, start=1):
                arguments = from_replies(arguments, replies)
                started = time.time()
                reply = session.request('tools/call', {'name': tool, 'arguments': arguments})
                payload = {'name': f'{index:02} {tool}', 'arguments': arguments,
                           'seconds': round(time.time() - started, 3), 'refused': reply['isError']}
                for part in reply['content']:
                    if part['type'] == 'image':
                        payload['frame'] = str(artifact / 'frames' / f'{index:02}-{tool}.png')
                        Path(payload['frame']).write_bytes(base64.b64decode(part['data']))
                        frames.append(payload['frame'])
                    else:
                        try:
                            payload['reply'] = json.loads(part['text'])
                        except ValueError:
                            payload['reply'] = part['text']
                replies[index] = payload.get('reply')
                must_refuse = expect == ['refused']
                assertion = {'type': 'assertion', 'passed': payload['refused'] == must_refuse
                             and (must_refuse or expected_result(tool, arguments, payload.get('reply'))),
                             'name': f"{payload['name']} {'is refused' if must_refuse else 'answers'}"}
                assertions.append(assertion)
                events.write(json.dumps({'type': 'action', 'payload': payload}) + '\n')
                events.write(json.dumps(assertion) + '\n')
                print(f"{'pass' if assertion['passed'] else 'FAIL'}: {assertion['name']} {payload['seconds']}s")
    except Exception as error:
        # A drive that dies still writes its report, so `latest.txt` never
        # names an older pass.
        died = {'type': 'assertion', 'passed': False, 'name': f'the drive finishes: {error}'}
        assertions.append(died)
        with open(artifact / 'events.ndjson', 'a') as events:
            events.write(json.dumps(died) + '\n')
        print(f"FAIL: {died['name']}")
    finally:
        cleanup_errors = []
        if session:
            try:
                app_pid = session.app_pid()
            except Exception as error:
                cleanup_errors.append(str(error))
            try:
                session.close()
            except Exception as error:
                cleanup_errors.append(str(error))
        if app_pid:
            try:
                os.kill(app_pid, signal.SIGTERM)
                try:
                    wait_for('the app to exit', lambda: subprocess.run(['kill', '-0', str(app_pid)],
                               capture_output=True).returncode != 0, args.timeout)
                except RuntimeError:
                    os.kill(app_pid, signal.SIGKILL)
                    wait_for('the killed app to exit', lambda: subprocess.run(['kill', '-0', str(app_pid)],
                               capture_output=True).returncode != 0, args.timeout)
            except ProcessLookupError:
                pass
            except Exception as error:
                cleanup_errors.append(str(error))
        for error in cleanup_errors:
            assertions.append({'type': 'assertion', 'passed': False,
                               'name': f'cleanup finishes: {error}'})
        shutil.rmtree(data_root, ignore_errors=True)
    failed = [assertion['name'] for assertion in assertions if not assertion['passed']]
    status = 'fail' if failed else 'pass'
    (artifact / 'report.json').write_text(json.dumps({
        'scenario': 'mcp-drive', 'status': status, 'artifact_dir': str(artifact),
        'first_failure': failed[0] if failed else None, 'assertions': assertions, 'frames': frames,
    }, indent=2))
    (scenario_dir / 'latest.txt').write_text(str(artifact))
    print(f'tools={len(tools)}\ncalls={len(assertions)}\nfailed={len(failed)}\n'
          f'status={status}\nartifact={artifact}')
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
