#!/usr/bin/env bash
# Run the private wake command from .env.windows, then wait for Windows SSH.
# How the PC wakes is machine-specific, so the repo only runs the command.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"

if [[ -z "${SNAPPYREF_WINDOWS_WAKE_CMD:-}" ]]; then
  echo 'Set SNAPPYREF_WINDOWS_WAKE_CMD in .env.windows to the command that wakes the PC.' >&2
  exit 2
fi
timeout_seconds="${SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS:-120}"
if [[ ! "$timeout_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo 'SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS must be a positive integer.' >&2
  exit 2
fi

if ! bash -c "$SNAPPYREF_WINDOWS_WAKE_CMD"; then
  echo 'The wake command failed.' >&2
  exit 1
fi

echo 'Wake command ran. Waiting for Windows SSH.'
python3 - "$SNAPPYREF_WINDOWS_HOST" "$timeout_seconds" <<'PYTHON'
import subprocess
import sys
import time

host, seconds = sys.argv[1:]
deadline = time.monotonic() + int(seconds)
while (remaining := deadline - time.monotonic()) > 0:
    try:
        result = subprocess.run(
            ['ssh', '-n', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=5',
             '-o', 'ConnectionAttempts=1', '--', host, 'exit'],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL, timeout=min(5, remaining))
        if result.returncode == 0:
            print('Windows SSH is reachable.')
            sys.exit(0)
    except subprocess.TimeoutExpired:
        pass
    time.sleep(min(1, max(0, deadline - time.monotonic())))
print(f'Windows SSH did not become reachable within {seconds} seconds.', file=sys.stderr)
sys.exit(1)
PYTHON
