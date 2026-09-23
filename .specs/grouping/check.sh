#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
quint typecheck .specs/grouping/board.qnt
quint test .specs/grouping/board.qnt
python3 - <<'PY'
import os
import re
import subprocess
from pathlib import Path
path = '.specs/grouping/board.qnt'
source = Path(path).read_text()
invariants = re.findall(r'^\s*val (inv_\w+)', source, re.MULTILINE)
witnesses = re.findall(r'^\s*val (wit_\w+)', source, re.MULTILINE)
result = subprocess.run(['quint', 'run', path, '--max-samples=5000', '--max-steps=30',
    '--seed=0x1', '--verbosity=1', '--invariants', *invariants, '--witnesses', *witnesses],
    text=True, capture_output=True, env={k:v for k,v in os.environ.items() if k != 'FORCE_COLOR'})
print(result.stdout, end='')
print(result.stderr, end='')
result.check_returncode()
observed = dict(re.findall(r'(wit_\w+)\W+was witnessed in\W+(\d+)', result.stdout))
missing = [name for name in witnesses if int(observed.get(name, 0)) == 0]
if missing:
    raise SystemExit('Unreached coverage cases: ' + ', '.join(missing))
PY
