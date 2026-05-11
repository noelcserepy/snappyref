# Relocate semantic model out of app bundle

The semantic search model (siglip2_base_patch16_224_int8) currently ships inside `SnappyRef.app/Contents/Resources/models/` on macOS and `SnappyRef/models/` on Windows, embedded in the release zip. SnappyRef will move the model out of the app bundle into a per-user **Local Model Store** so that app code and user-acquired data live in separate, independently-replaceable locations.

The model is large (~hundreds of MB) relative to the rest of the app, and it does not change between most releases. Bundling it forces every release zip to be hundreds of MB regardless of whether anything about the model changed, and forces auto-update payloads to redownload data the user already has. Moving the model out of the bundle reduces typical update payload size by roughly 10×, simplifies the bundle-swap step of auto-update, and aligns the model with other user-acquired data the app already keeps outside the bundle.

This is a precondition for ADR-0015 (automatic app updates): the updater cannot reasonably swap an entire macOS `.app` bundle without losing or redownloading the bundled model on every update.

## Decision

- Move the model directory out of the bundle to a **Local Model Store** at `<install_root>/models/`, where `install_root` is resolved by [src/library_location.rs](../../src/library_location.rs):
  - macOS: parent directory of `SnappyRef.app`.
  - Windows: parent directory of `snappyref.exe`.
- The Local Model Store is a sibling of the existing `library/` directory, following the same portable-folder convention SnappyRef already uses for user data.
- Release zips no longer bundle the model. Continue to publish a separate full-install zip (app + model in `models/`) for fresh installs that prefer offline-friendly first launch.
- First launch with no Local Model Store performs a one-time fetch from the hosted model bundle URL, verified by sha256, into `<install_root>/models/`.
- First launch *with* an in-bundle model present (existing installs upgrading to this version) performs a one-shot migration: move the in-bundle model files into `<install_root>/models/`, then continue startup. The migration runs once per install and is idempotent.
- The Local Model Store is replaceable cache-like data, not user-owned canvas data. Deleting it triggers a refetch on next launch.

## Consequences

- App-only release zips drop from ~400 MB to ~50 MB.
- Fresh installs require network access on first launch to fetch the model. Acceptable because semantic search itself requires the model and the user is expected to be online for first setup; document this in `UPDATE.txt` and on the website download page.
- Diagnostic and reset flows must read the model from the new location. Any future "reset model" command operates on the Local Model Store, not the bundle.
- Deleting the Local Model Store triggers a refetch on next launch; it survives library reset and is unaffected by app updates.
- Existing users do not manually move files; the in-app migration handles it on the first launch after upgrade.
- The Local Model Store inherits the existing portable-folder caveat: if the user drops `SnappyRef.app` directly into `/Applications` on macOS, `<install_root>` becomes admin-owned and writes will fail. `UPDATE.txt` and the website download page must instruct users to keep the SnappyRef folder in a writable location (e.g., `~/SnappyRef/`, `~/Documents/SnappyRef/`).
- Without ADR-0015 (auto-update), this change still stands on its own — smaller release zips and a cleaner separation between app and acquired data.
