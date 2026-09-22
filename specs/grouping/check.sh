#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
# Check the plan. Implementation trace replay belongs to implement-quint-spec.
quint typecheck specs/grouping/spec.qnt
quint test specs/grouping/spec.qnt
python3 - <<'PYTHON'
import re
import subprocess
from pathlib import Path

source = Path("specs/grouping/spec.qnt").read_text()
witnesses = re.findall(r"^\s*val (wit_\w+)", source, re.MULTILINE)
result = subprocess.run([
    "quint", "run", "specs/grouping/spec.qnt", "--max-samples=10000",
    "--max-steps=60", "--seed=0x1", "--verbosity=1", "--invariants",
    *re.findall(r"^\s*val (inv_\w+)", source, re.MULTILINE), "--witnesses",
    *witnesses,
], text=True, capture_output=True)
print(result.stdout, end="")
print(result.stderr, end="")
result.check_returncode()
observed = dict(re.findall(r"(wit_\w+) was witnessed in (\d+) trace", result.stdout))
missing = [name for name in witnesses if int(observed.get(name, 0)) == 0]
if missing:
    raise SystemExit("Unreached coverage cases: " + ", ".join(missing))
PYTHON
