# Automatic app updates

SnappyRef will perform automatic in-app updates on macOS and Windows. The app polls a static signed update manifest in the background, surfaces an **Available Update** through a pill in the sidebar, and on user confirmation downloads, verifies, swaps, and relaunches the app via a small helper binary. This supersedes ADR-0006 (manual updates for v1).

Manual updates impose meaningful friction (visit website, download zip, quit app, replace files, relaunch) that discourages users from staying current and slows feature delivery. With release packaging, the model lifecycle (ADR-0014), and the user-owned library contract now stable enough to make safe assumptions, an in-app updater can carry the upgrade cost for the user. The design below favors smallest viable surface area: static hosting, signed manifest, helper-binary swap, no rollout logic, no telemetry.

## Decision

### Distribution

- A separate public GitHub repository `snappyref-dist` hosts release artifacts. The private source repo stays private.
- CI publishes per-platform app zips and a signed `latest.json` manifest as release assets on `snappyref-dist`.
- The app fetches `latest.json` directly from the GitHub release URL. No website cron, no API route.
- Each platform zip contains a single top-level `SnappyRef/` folder so the user's "drag the folder anywhere" model is uniform across macOS and Windows. On macOS the folder contains `SnappyRef.app`; on Windows it contains `snappyref.exe`. The helper binary ships inside the folder/bundle.
- Two zip flavors per platform:
  - **App-only zip** (`snappyref-mac-{tag}.zip`, `snappyref-windows-{tag}.zip`): used for auto-update payloads and as the default download. Excludes the model.
  - **Full-install zip** (`snappyref-mac-{tag}-full.zip`, `snappyref-windows-{tag}-full.zip`): includes `models/` at the folder root. Linked from the website for users who want an offline-friendly first install.

### Manifest

- `latest.json` schema includes `channel`, `version`, per-platform `{ url, sha256, size }`, release-notes URL, and a `signature` field over the canonical manifest bytes.
- Only `channel: "stable"` exists at runtime. The field is reserved for future beta/insider channels without requiring a schema migration.
- Manifest is signed with an ed25519 keypair. The public key is compiled into the app as a constant. The private key lives as a GitHub Actions secret used by the publish workflow.

### Client behavior

- The client checks for updates after **Settled Idle** following startup (never blocks startup), then every 6 hours while the app remains running.
- Discovery (fetching the manifest) is separate from download (fetching the zip). Discovery is automatic; download is gated on explicit user click.
- The client verifies the manifest signature before trusting any field. It verifies the downloaded zip's sha256 against the manifest before applying.
- A failed check (no network, GitHub unreachable, signature mismatch) backs off exponentially up to 6 hours and never surfaces a popup. The pill stays in its last known state.
- The ping is a plain GET with no parameters, no body, no telemetry. The client filters per-platform fields locally.

### UI

- An update pill appears in the sidebar near the bottom when an Available Update exists.
- First click opens a small popover showing the new version number, a release-notes link, and `Install` / `Later` buttons.
- During download the pill becomes a progress indicator in place; on completion it flips to `Restart`.
- The Settings tab includes an `About / Updates` section: current version, last-checked time, manual `Check for updates`, and a `Rollback to previous version` button (visible only when a backup exists).

### Install mechanics

- Downloaded zip is staged inside the install folder at `<install_root>/updates/staging-{version}/`, alongside `library/` and `models/`. This keeps all SnappyRef-related state inside the user's portable folder; no system app-data directories are used.
- The `snappyref-updater` helper binary ships inside the bundle/folder alongside the main executable (`SnappyRef.app/Contents/MacOS/snappyref-updater` on macOS, `SnappyRef/snappyref-updater.exe` on Windows).
- On user confirmation: the app saves the **User Session**, stops worker child processes (embedding helper, image preparation helper), writes a pending-update descriptor into `<install_root>/updates/`, launches the helper binary, and exits.
- The helper waits for the parent process to exit, performs the platform-specific swap inside `<install_root>`, launches the new app, and exits:
  - **macOS**: rename current `SnappyRef.app` to `SnappyRef.app.previous`; move the staged bundle into place at `<install_root>/SnappyRef.app`; `open SnappyRef.app`.
  - **Windows**: rename current `snappyref.exe` to `snappyref.exe.old`; write new exe alongside; launch new exe.
- The helper explicitly strips the `com.apple.quarantine` xattr from the staged bundle before swap as defense-in-depth. In-process `reqwest` downloads do not set the xattr, so Gatekeeper does not engage on the swapped bundle. Real Developer ID signing remains a future, separate decision.
- After a successful launch of the new version, the staging directory `<install_root>/updates/staging-{version}/` is removed; the `.previous` / `.old` backup is retained until the next update overwrites it.

### Rollback

- Exactly one prior version is retained as `SnappyRef.app.previous` / `snappyref.exe.old`. Each update overwrites the prior backup.
- Rollback is exposed two ways: a CLI flag (`snappyref --rollback`) for the case where the new version cannot launch, and a `Rollback to previous version` button in the Settings tab. No automatic watchdog rollback in v1.

### What is preserved across updates

- The **Local Model Store** (ADR-0014) is outside the bundle and untouched by updates.
- The user-owned `library/` is outside the bundle and untouched by updates.
- The User Session is saved before the swap and reloaded after relaunch.

## Consequences

- A new public `snappyref-dist` repo and an ed25519 signing keypair must be created and provisioned in CI before the first auto-update-eligible release.
- Losing the signing private key bricks the update channel. Mitigation: keep an encrypted backup of the key, plus the ability to ship a manually-installed version that rotates the embedded public key.
- The website's download page should link to the latest release on `snappyref-dist` rather than to manually-managed UploadThing URLs; UploadThing can be retired for release hosting.
- Without Developer ID signing or Notarization, the first manual install on macOS still requires the right-click-open Gatekeeper bypass. Auto-updates are silent after that. Real platform code signing is a separate future ADR.
- The auto-updater depends on ADR-0014 (relocated model). A release that included auto-update support but kept the in-bundle model would either move ~400 MB per update or break model availability after swap.
- All update state lives in `<install_root>`. If the user has placed `SnappyRef.app` directly into `/Applications` on macOS, the install root is admin-owned and the updater cannot write `updates/` or swap the bundle. `UPDATE.txt` and the website download page must instruct users to keep the SnappyRef folder in a writable location. A future enhancement may detect this case and surface a clear error.
- Discovery is opportunistic; there is no guarantee of immediate update propagation. A critical fix may take up to 6 hours per user to surface. Acceptable for the user base v1 targets; force-update can be revisited later.
- No telemetry is collected. Adoption metrics are not available; this is intentional for v1 and can be reconsidered with explicit consent later.
- ADR-0006 (manual updates for v1) is superseded by this decision.
