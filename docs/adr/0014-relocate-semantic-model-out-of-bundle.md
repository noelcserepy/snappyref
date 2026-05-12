# Relocate semantic model out of app bundle

The semantic search model (siglip2_base_patch16_224_int8) currently ships inside `SnappyRef.app/Contents/Resources/models/` on macOS and `SnappyRef/models/` on Windows, embedded in the release zip. SnappyRef will move the model out of the app bundle into a per-user **Local Model Store** so that app code and user-acquired data live in separate, independently-replaceable locations.

The model is large (~hundreds of MB) relative to the rest of the app, and it does not change between most releases. Bundling it forces every release zip to be hundreds of MB regardless of whether anything about the model changed, and forces auto-update payloads to redownload data the user already has. Moving the model out of the bundle reduces typical update payload size by roughly 10×, simplifies the bundle-swap step of auto-update, and aligns the model with other user-acquired data the app already keeps outside the bundle.

This is a precondition for ADR-0015 (automatic app updates): the updater cannot reasonably swap an entire macOS `.app` bundle without losing or redownloading the bundled model on every update.

## Decision

- Move the model directory out of the bundle to a **Local Model Store** at `<install_root>/models/`, where `install_root` is resolved by [src/library_location.rs](../../src/library_location.rs):
  - macOS: parent directory of `SnappyRef.app`.
  - Windows: parent directory of `snappyref.exe`.
- The Local Model Store is a sibling of the existing `library/` directory, following the same portable-folder convention SnappyRef already uses for user data.
- Release zips no longer bundle the model, and SnappyRef no longer publishes full-install zips. The Semantic Search assets are published once as a long-lived standalone `snappyref-semantic-search.zip` release asset on `snappyref-dist` and reused by app versions until the search assets change.
- The standalone asset lives on a dedicated `semantic-search` release tag in `snappyref-dist`. A manually-triggered GitHub Actions workflow builds, validates, and uploads that asset so setting up Semantic Search distribution is a one-button operation. The workflow refuses to replace an existing asset unless explicitly told to do so.
- The Semantic Search distribution workflow prints the compiled app descriptor values in its run summary: URL, sha256, byte size, and model key.
- Release app builds require those descriptor values as workflow inputs and compile them into the binary. A release build must fail rather than ship an app whose Semantic Search install action has no sha256 or byte-size contract.
- App build workflows upload the descriptor as a short-lived artifact beside the app artifact. Release packaging must verify that every selected platform build has the descriptor artifact and that selected platform descriptors match before publishing zips.
- Semantic Search assets are immutable from the app's perspective: an app version downloads the exact URL and sha256 it was built with. A materially changed asset uses a new release tag and ships through a normal app update, while older assets remain available for older app versions.
- First launch with no Local Model Store shows a one-time prompt asking whether to enable **Semantic Search** by downloading the required search assets, verified by sha256, into `<install_root>/models/`. Choosing "Not Now" is remembered and the app does not ask again on launch.
- The first-launch Semantic Search prompt appears only after the normal app surface is usable and storage is available. If storage fails, the prompt is skipped.
- If the user declines the first-launch prompt, **Semantic Search** remains installable from the search palette when the user presses Command+F / Control+F.
- The app uses a compiled-in package descriptor for v1: URL, sha256, byte size, and model key. A future search-asset change ships through a normal app update that rotates those constants.
- `snappyref-semantic-search.zip` contains `models/<model_key>/...` so install can stage into the install root and validate the exact Local Model Store layout before promotion.
- First launch *with* an in-bundle model present (existing installs upgrading to this version) performs a one-shot migration: move the in-bundle model files into `<install_root>/models/`, then continue startup. The migration runs once per install and is idempotent.
- The Local Model Store is replaceable cache-like data, not user-owned canvas data. Deleting it triggers a refetch on next launch.
- The Settings Tab exposes Semantic Search install status and lets the user install, retry, or remove the Local Model Store. Removing the Local Model Store does not delete stored embeddings; reinstalling compatible search assets can reuse them.
- Removing Semantic Search is available only from Settings and requires a small confirmation because it creates a future redownload cost, even though it does not delete user-owned library data.
- Download or extraction failures stay inline in the startup prompt, search palette, or Settings Tab with a Retry action. They do not open a separate error modal.
- Active Semantic Search downloads can be cancelled from the startup prompt and Settings Tab. Cancellation deletes the partial zip/staging directory and returns Semantic Search to the not-installed state.
- After the user confirms installation from the startup prompt, the prompt dismisses and the download continues in the background. The app remains usable; progress is surfaced through Semantic Search UI surfaces, and indexing/backfill begins only after installation completes.
- Once Semantic Search installation completes, SnappyRef enables the existing idle-prioritized embedding backfill. It does not block the app with a whole-library preparation phase.

## Consequences

- App-only release zips drop from ~400 MB to ~50 MB.
- Fresh installs do not require network access before the app is usable. **Semantic Search** requires a later explicit download from the long-lived standalone asset.
- Diagnostic and reset flows must read the model from the new location. Any future "reset model" command operates on the Local Model Store, not the bundle.
- Deleting the Local Model Store triggers a refetch on next launch; it survives library reset and is unaffected by app updates.
- Existing users do not manually move files; the in-app migration handles it on the first launch after upgrade.
- The Local Model Store inherits the existing portable-folder caveat: if the user drops `SnappyRef.app` directly into `/Applications` on macOS, `<install_root>` becomes admin-owned and writes will fail. `UPDATE.txt` and the website download page must instruct users to keep the SnappyRef folder in a writable location (e.g., `~/SnappyRef/`, `~/Documents/SnappyRef/`).
- Without ADR-0015 (auto-update), this change still stands on its own — smaller release zips and a cleaner separation between app and acquired data.
