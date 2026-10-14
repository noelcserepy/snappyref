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
- contributor names, branch names, commit hashes, or internal architecture terminology when a plain user-facing phrase is clearer
- promising a fix more broadly than the PR evidence supports

## Inputs

- Release number: use the user's provided release number. If missing, infer it from the latest unreleased version/tag context when obvious; otherwise ask for it.
- Previous release: select the latest published app release from `noelcserepy/snappyref`, excluding drafts and `semantic-search`. Its tag is the source commit boundary. Never use `targetCommitish` as a source boundary.
- Target commit: resolve the new release tag or intended release ref to its exact commit. Use `HEAD` only when that is the intended release code.
- Change set: commits included in the target but absent from the previous release, with their associated PRs.
- Do not use the PR that updates the version number as the release boundary. Version bumps may happen before or after release work and are not reliable.

## Gathering PRs

1. Select the latest published app release:

```sh
git status --short
gh api --paginate repos/noelcserepy/snappyref/releases --slurp | \
  jq 'add | map(select(.draft == false and .published_at != null and .tag_name != "semantic-search" and any(.assets[]?; .name | test("^snappyref-(mac|windows)-.*\\.zip$")))) | sort_by(.published_at) | last | {tag_name, published_at, html_url}'
```

Confirm the selected release has app ZIP assets, not just the standalone model
asset. Inspect its release metadata and assets if the tag is unfamiliar.

2. Resolve that exact tag to its commit:

```sh
gh release view <published-app-tag> --repo noelcserepy/snappyref --json tagName,publishedAt,assets,url
git fetch origin tag <published-app-tag>
git rev-parse '<published-app-tag>^{commit}'
```

Confirm `origin` is `noelcserepy/snappyref` before fetching. Record the
release URL/tag and resolved commit. If the tag is missing, stop and resolve
the missing provenance. Do not substitute a release publication date or
version-bump PR.

3. Enumerate the complete commit range:

```sh
git merge-base --is-ancestor <previous-release-commit> <target-commit>
git log --reverse --format='%H %s' <previous-release-commit>..<target-commit>
```

If the ancestor check fails, inspect the release branches before choosing the
comparison range. Include merge, squash, rebase, and direct commits. Publication
dates do not determine which changes shipped.

4. Resolve associated PRs from the commits, using merge or squash subjects when
they identify the PR. For a commit without a reliable PR link:

```sh
gh api --paginate 'repos/noelcserepy/snappyref/commits/<commit-sha>/pulls' \
  --jq '.[] | select(.merged_at != null) | {number, title, merge_commit_sha, html_url}'
```

Deduplicate PR numbers across commits. A PR association is supporting metadata;
the commit range remains authoritative. Check reverts and partial inclusion
before describing a whole PR as shipped. Keep direct commits without PRs in the
review. If GitHub metadata is unavailable, inspect the local commits and diffs.

5. Inspect PR details when the title does not establish the user-facing effect:

```sh
gh pr view <number> --repo noelcserepy/snappyref --json title,body,files,labels,mergedAt,url
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

- Confirm the previous release and target commits, account for the complete range, and deduplicate the associated PRs.
- Re-read the bullets and remove anything that exposes internal work without a user-visible consequence.
- Make sure every bullet would make sense to a non-developer using SnappyRef.
- Make sure the final answer is the patchnotes markdown, not a process report, unless the user explicitly asks for methodology.
