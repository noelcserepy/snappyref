#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/windows-env.sh
source "$script_dir/windows-env.sh"

cd "$SNAPPYREF_REPO_ROOT"

git ls-files -co --exclude-standard -z \
  | tar --null -T - -cf - \
  | ssh -o BatchMode=yes "$SNAPPYREF_WINDOWS_HOST" \
      "powershell -NoProfile -Command \"cd '$SNAPPYREF_WINDOWS_REPO'; tar -xf -\""
