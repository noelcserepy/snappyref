#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="${SNAPPYREF_WINDOWS_ENV:-"$repo_root/.env.windows"}"
if [[ -z "${SNAPPYREF_WINDOWS_ENV:-}" && ! -f "$env_file" ]]; then
  # A linked worktree has no copy; the main checkout holds the private file.
  common_dir="$(git -C "$repo_root" rev-parse --path-format=absolute --git-common-dir)"
  env_file="$(dirname "$common_dir")/.env.windows"
fi

if [[ -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$env_file"
  set +a
fi

: "${SNAPPYREF_WINDOWS_HOST:?Set SNAPPYREF_WINDOWS_HOST in .env.windows or the environment.}"

export SNAPPYREF_WINDOWS_HOST
export SNAPPYREF_WINDOWS_REPO
