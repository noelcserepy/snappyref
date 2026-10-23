#!/usr/bin/env bash
# Opens one issue per CI job when that job fails on main, or comments on its
# open one. Takes the job name, such as macOS or Windows. Needs GH_TOKEN and
# GH_REPO, plus the run variables Actions sets.
set -euo pipefail

title="${1:?usage: report-ci-failure.sh <job>} CI is red on main"
body="Run: ${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}
Commit: ${GITHUB_SHA}"

# The search narrows by title on the server, so the open report is found
# however many newer issues exist. The filter keeps only the exact title.
open_issue=$(gh issue list --state open --search "\"${title}\" in:title" \
  --json number,title \
  --jq "map(select(.title == \"${title}\")) | .[0].number // empty")

if [[ -n "$open_issue" ]]; then
  gh issue comment "$open_issue" --body "$body"
else
  gh issue create --title "$title" --body "$body"
fi
