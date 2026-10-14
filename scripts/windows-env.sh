#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="${SNAPPYREF_WINDOWS_ENV:-"$repo_root/.env.windows"}"

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
fi

: "${SNAPPYREF_WINDOWS_HOST:?Set SNAPPYREF_WINDOWS_HOST in .env.windows or the environment.}"

export SNAPPYREF_WINDOWS_HOST
export SNAPPYREF_WINDOWS_REPO
