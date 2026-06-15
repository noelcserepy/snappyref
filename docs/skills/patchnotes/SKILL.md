---
name: patchnotes
description: |
  Write user-facing SnappyRef patchnotes from the PRs merged since the last released version. Use when asked to write patchnotes, release notes, changelog copy, or a user-facing summary for a SnappyRef release.
user-invocable: true
argument-hint: "[release-number]"
---

# Patchnotes

Use this skill to write concise user-facing patchnotes for a SnappyRef release. The output is chat-only markdown unless the user explicitly asks for a file.

## Goal

Patchnotes should answer: "What changed that a SnappyRef user would notice or care about?"

Prefer:

- new or improved workflows
- visible UI and interaction changes
- import, search, canvas, update, library, board, shortcut, and settings behavior
- stability, reliability, performance, and compatibility improvements
- fixes for crashes, broken flows, data preservation, updater issues, platform-specific behavior, or confusing UI states

Avoid:

- internal refactors, architecture, implementation details, test-only work, CI, release tooling, dependency bumps, docs-only work, and agent/process changes
- PR numbers in the final copy unless the user asks for traceability
- contributor names, branch names, commit hashes, or internal code/ADR terminology when a plain user-facing phrase is clearer
- promising a fix more broadly than the PR evidence supports

## Inputs

- Release number: use the user's provided release number. If missing, infer it from the latest unreleased version/tag context when obvious; otherwise ask for it.
- Previous release: find the most recent actual GitHub Release and the exact commit/tag it points to. Use that commit as the boundary.
- Change set: PRs merged after the latest actual release commit and included in the new release.
- Do not use the PR that updates the version number as the release boundary. Version bumps may happen before or after release work and are not reliable.

## Gathering PRs

1. Check the current branch, tags, and latest release:

```sh
git status --short
git tag --sort=-creatordate | head -20
gh release list --limit 20
```

2. Resolve the latest actual release to a commit before collecting PRs:

```sh
gh release view <latest-release-tag> --json tagName,targetCommitish,publishedAt,url
git rev-list -n 1 <latest-release-tag>
```

Use `git rev-list -n 1 <latest-release-tag>` as the previous-release commit when the release tag exists locally. If the tag is missing, fetch tags or use GitHub release metadata to resolve the target commit. Do not substitute a version-bump commit or version-bump PR for this boundary.

3. Prefer GitHub release metadata when available:

```sh
gh pr list --state merged --base main --search "merged:>=YYYY-MM-DD" --json number,title,body,mergedAt,labels,url
```

Use the latest release `publishedAt` date only as a search accelerator. The authoritative inclusion boundary is still the latest release commit.

4. If GitHub metadata is unavailable, fall back to local git history from the release commit:

```sh
git log <previous-release-commit>..HEAD --merges --first-parent --oneline
git log <previous-release-commit>..HEAD --oneline
```

5. For ambiguous PR titles, inspect the PR body or changed files:

```sh
gh pr view <number> --json title,body,files,labels,mergedAt,url
```

Do not include a change just because it appears in git history. Include it only if it has a user-facing effect or clear stability/performance value.

## Classification

Use these sections, omitting any section with no entries:

- `Features`: new user-visible capabilities or substantial workflow additions.
- `Fixes`: bug fixes, crash fixes, platform compatibility fixes, updater/data-preservation fixes, and corrected behavior.
- `Improvements`: polish, usability, performance, reliability, stability, clearer defaults, and refined behavior that improves an existing capability.

When a change could fit multiple sections, choose the section that matches how the user experiences it. For example, a rewritten import pipeline belongs in `Improvements` if the user benefit is faster or more reliable importing, not in `Features`.

## Writing Style

- Use short bullets.
- Lead with the benefit, not the implementation.
- Keep wording concrete and modest.
- Combine tiny related PRs into one bullet.
- Say "Improved stability..." or "Improved reliability..." only when the PR evidence supports it.
- Prefer SnappyRef domain language where it is user-facing: Library, Board, Semantic Search, canvas, placements, textboxes, update.
- Avoid deep domain terms in release copy when simpler wording works: say "keeps your canvas responsive while dragging" instead of "storage-free interaction frames."

## Output Format

Write only this markdown structure in chat. Leave out sections that do not apply.

```markdown
# SnappyRef <release-number>

## Features

- ...

## Fixes

- ...

## Improvements

- ...
```

If there are no user-facing changes, say so briefly instead of padding the release notes with internal work.

## Final Check

Before responding:

- Confirm the latest actual release tag/commit used as the boundary and the PR range collected from it.
- Re-read the bullets and remove anything that exposes internal work without a user-visible consequence.
- Make sure every bullet would make sense to a non-developer using SnappyRef.
- Make sure the final answer is the patchnotes markdown, not a process report, unless the user explicitly asks for methodology.
