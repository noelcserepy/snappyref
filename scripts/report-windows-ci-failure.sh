#!/usr/bin/env bash
# Opens one issue when the Windows CI job fails on main, or comments on the
# open one. Needs GH_TOKEN and GH_REPO, plus the run variables Actions sets.
set -euo pipefail

title="Windows CI is red on main"
body="Run: ${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}
Commit: ${GITHUB_SHA}"

open_issue=$(gh issue list --state open --limit 200 --json number,title \
  --jq "map(select(.title == \"${title}\")) | .[0].number // empty")

if [[ -n "$open_issue" ]]; then
  gh issue comment "$open_issue" --body "$body"
else
  gh issue create --title "$title" --body "$body"
fi
