# Release Candidate Checklist

Use this checklist when cutting the first release that supports the manual portable update flow.

## Candidate Prep

1. Choose the release version.
2. Bump the package version to the chosen release version.
3. Confirm release artifact names include the tag or version expected by the release workflow.
4. Draft release notes with the manual update instructions from `UPDATE.txt`.
5. Confirm downgrade remains documented as unsupported after forward migration.

## Tag And Artifact Verification

1. Create the release tag in a dry-run branch or rehearsal tag before the final tag.
2. Let the tag-triggered release workflow build Windows and macOS artifacts.
3. Confirm both uploaded zips pass the release archive contract check.
4. Confirm both uploaded zips contain `UPDATE.txt`, the app binary, and packaged models.
5. Confirm neither uploaded zip contains `library/`, `library.db`, `originals/`, or `thumbs/`.
6. Run the mandatory real-zip update smoke checklist before publishing the release broadly.

## Pass Criteria

- Version bump matches the release tag.
- Artifact names match the expected versioned naming.
- Release notes include manual update instructions.
- GitHub release contains Windows and macOS zips.
- Final artifacts pass archive contract checks.
- Real-zip update smoke passes on the target release candidate.
