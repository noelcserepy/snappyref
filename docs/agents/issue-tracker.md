# Issue Tracker

GitHub Issues are the canonical implementation backlog for SnappyRef. Use the `gh` CLI from inside this repo.

## Canonical backlog

- PRDs created by `to-prd` should become GitHub issues.
- Implementation slices created by `to-issues` should become GitHub issues in dependency order.
- Issues ready for autonomous implementation should carry `enhancement` or `bug` plus the appropriate triage state label from `triage-labels.md`.

## Private draft backlog

Use `.scratch/backlog/` for private notes, Obsidian exports, rough PRD drafts, and issue drafts that are not ready for GitHub yet.

This directory is repo-local private context and must remain gitignored. Do not publish raw private notes. Convert them into PRDs or issue bodies first.

Suggested layout:

```text
.scratch/backlog/
├── <topic-slug>/
│   ├── notes.md
│   ├── PRD-draft.md
│   └── issues/
│       ├── 01-<slice>.md
│       └── 02-<slice>.md
```

## Commands

- Create issue: `gh issue create --title "..." --body "..."`
- Read issue: `gh issue view <number> --comments`
- List issues: `gh issue list --state open --json number,title,labels,updatedAt`
- Comment: `gh issue comment <number> --body "..."`
- Apply labels: `gh issue edit <number> --add-label "..."`
- Remove labels: `gh issue edit <number> --remove-label "..."`
- Close: `gh issue close <number> --comment "..."`

## Skill wording

When a skill says "publish to the issue tracker", create a GitHub issue unless the user explicitly says to keep the output private.

When a skill says "fetch the relevant ticket", use `gh issue view <number> --comments`.
