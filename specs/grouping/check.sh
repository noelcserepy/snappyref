#!/bin/sh
set -eu
cd "$(dirname "$0")/../.."
quint typecheck specs/grouping/spec.qnt
quint test specs/grouping/spec.qnt
python3 - <<'PYTHON'
import re
import subprocess
from pathlib import Path
source = Path("specs/grouping/spec.qnt").read_text()
subprocess.run([
    "quint", "run", "specs/grouping/spec.qnt", "--max-samples=10000",
    "--max-steps=60", "--seed=0x1", "--verbosity=1", "--invariants",
    *re.findall(r"^\s*val (inv_\w+)", source, re.MULTILINE), "--witnesses",
    *re.findall(r"^\s*val (wit_\w+)", source, re.MULTILINE),
], check=True)
PYTHON
mkdir -p specs/traces
quint run specs/grouping/spec.qnt --mbt --n-traces=200 --max-samples=200 --max-steps=20 --seed=0x1 --out-itf=specs/traces/short.itf.json --verbosity=0
quint run specs/grouping/spec.qnt --mbt --n-traces=50 --max-samples=50 --max-steps=60 --seed=0x2 --out-itf=specs/traces/long.itf.json --verbosity=0
cargo test --locked grouping::tests -- --nocapture
