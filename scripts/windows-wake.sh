#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"

if [[ -z "${SNAPPYREF_WINDOWS_WAKE_RELAY:-}" || -z "${SNAPPYREF_WINDOWS_MAC:-}" ]]; then
  echo 'Set SNAPPYREF_WINDOWS_WAKE_RELAY and SNAPPYREF_WINDOWS_MAC in .env.windows.' >&2
  exit 2
fi
mac="$SNAPPYREF_WINDOWS_MAC"
broadcast="${SNAPPYREF_WINDOWS_WAKE_BROADCAST:-255.255.255.255}"
timeout_seconds="${SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS:-120}"
if [[ ! "$mac" =~ ^[[:xdigit:]]{2}([:-][[:xdigit:]]{2}){5}$ ]]; then
  echo 'SNAPPYREF_WINDOWS_MAC must contain six hexadecimal byte pairs.' >&2
  exit 2
fi
if ! python3 -c 'import ipaddress, sys; ipaddress.IPv4Address(sys.argv[1])' "$broadcast" >/dev/null 2>&1; then
  echo 'SNAPPYREF_WINDOWS_WAKE_BROADCAST must be an IPv4 address.' >&2
  exit 2
fi
if [[ ! "$timeout_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo 'SNAPPYREF_WINDOWS_WAKE_TIMEOUT_SECONDS must be a positive integer.' >&2
  exit 2
fi

ssh -o BatchMode=yes -o ConnectTimeout=5 -o ConnectionAttempts=1 -- "$SNAPPYREF_WINDOWS_WAKE_RELAY" \
  "python3 - '$mac' '$broadcast'" <<'PY'
import socket
import sys

mac = bytes.fromhex(sys.argv[1].replace(':', '').replace('-', ''))
packet = b'\xff' * 6 + mac * 16
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender:
    sender.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sender.sendto(packet, (sys.argv[2], 9))
PY

echo 'Wake packet sent. Waiting for Windows SSH.'
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
