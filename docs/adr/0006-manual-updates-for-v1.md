# Manual updates for v1

SnappyRef v1 uses a manual update flow: the user quits the app, downloads a release zip, replaces app-owned release files, and keeps the existing user-owned `library/` directory in place. The app should not download, patch, replace, or restart itself automatically in v1, though an automatic updater remains a possible future decision once the release and user-data contracts are stable.

This keeps early distribution simple while packaging, model bundles, platform behavior, and update-preservation smoke tests are still settling. The trade-off is more user effort during updates, but it avoids committing to updater security, rollback, signing, and cross-platform patch behavior before the install shape is proven.

## Consequences

- Release artifacts must be shaped for direct file replacement.
- User-owned library data must stay outside release zips.
- A lightweight "update available" affordance may open a download page, but v1 should not perform self-update actions.
- If automatic updates are added later, they should be documented as a new decision that preserves the user-owned library contract.
