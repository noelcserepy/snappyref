#!/bin/sh
# Check every grouping module: typecheck, scenarios, then a random search that must see every
# witness. Each module runs in seconds. verify.sh is the exhaustive check; replay.sh is the
# implementation's.
set -eu
cd "$(dirname "$0")/../.."
for module in rules layout sizing board enablement; do
  quint typecheck "specs/grouping/$module.qnt"
done
for module in layout sizing board enablement; do
  quint test "specs/grouping/$module.qnt"
done
python3 - <<'PYTHON'
import os
import re
import subprocess
from pathlib import Path

# Algorithm modules pick their inputs in init, so one step is the whole search.
STEPS = {"layout": 1, "enablement": 1, "sizing": 30, "board": 30}
SAMPLES = {"layout": 5000, "enablement": 20000, "sizing": 3000, "board": 3000}
env = {k: v for k, v in os.environ.items() if k != "FORCE_COLOR"}
for module, steps in STEPS.items():
    path = f"specs/grouping/{module}.qnt"
    source = Path(path).read_text()
    invariants = re.findall(r"^\s*val (inv_\w+)", source, re.MULTILINE)
    witnesses = re.findall(r"^\s*val (wit_\w+)", source, re.MULTILINE)
    result = subprocess.run([
        "quint", "run", path, f"--max-samples={SAMPLES[module]}", f"--max-steps={steps}",
        "--seed=0x1", "--verbosity=1", "--invariants", *invariants, "--witnesses", *witnesses,
    ], text=True, capture_output=True, env=env)
    print(result.stdout, end="")
    print(result.stderr, end="")
    result.check_returncode()
    observed = dict(re.findall(r"(wit_\w+)\W+was witnessed in\W+(\d+)", result.stdout))
    missing = [name for name in witnesses if int(observed.get(name, 0)) == 0]
    if missing:
        raise SystemExit(f"{module}: unreached coverage cases: " + ", ".join(missing))
PYTHON
