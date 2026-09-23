#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
python3 - <<'PY'
import re
import subprocess
from pathlib import Path
path = '.specs/grouping/board.qnt'
invariants = re.findall(r'^\s*val (inv_\w+)', Path(path).read_text(), re.MULTILINE)
subprocess.run(['quint', 'verify', path, '--backend=tlc', '--invariants', *invariants], check=True)
PY
