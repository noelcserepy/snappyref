#!/usr/bin/env bash
set -euo pipefail

script="scripts/windows-launch-gui.sh"

grep -q 'Get-Process snappyref .* Stop-Process -Force' "$script"
grep -q 'if (-not (Get-Process snappyref' "$script"
grep -q 'Start-Process -FilePath' "$script"
grep -q '$time = "23:59"' "$script"
grep -q 'schtasks /Delete /TN $taskName /F 2>$null | Out-Null' "$script"
grep -q 'schtasks /Run /TN $taskName' "$script"

start_line=$(grep -n 'Start-Process -FilePath' "$script" | head -1 | cut -d: -f1)
run_line=$(grep -n 'schtasks /Run /TN $taskName' "$script" | head -1 | cut -d: -f1)
delete_line=$(grep -n 'schtasks /Delete /TN $taskName /F 2>$null | Out-Null' "$script" | tail -1 | cut -d: -f1)
if [ "$delete_line" -le "$start_line" ]; then
  echo "task cleanup must happen after the GUI action"
  exit 1
fi
if [ "$delete_line" -le "$run_line" ]; then
  echo "task cleanup must happen after the task is run"
  exit 1
fi
