# Automatic app updates

SnappyRef will perform automatic in-app updates on macOS and Windows. The app polls a static signed update manifest in the background, surfaces update state in Settings, and on user confirmation downloads, verifies, swaps, and relaunches the app. macOS uses a small helper binary for app-bundle swaps; Windows uses a single-exe self-replace path with a temporary runner copied from the current executable. This supersedes ADR-0006 (manual updates for v1).

Manual updates impose meaningful friction (visit website, download zip, quit app, replace files, relaunch) that discourages users from staying current and slows feature delivery. With release packaging, the model lifecycle (ADR-0014), and the user-owned library contract now stable enough to make safe assumptions, an in-app updater can carry the upgrade cost for the user. The design below favors smallest viable surface area: static hosting, signed manifest, platform-local swap mechanics, no rollout logic, no telemetry.

## Decision

### Distribution

- A separate public GitHub repository `snappyref-dist` hosts release artifacts. The private source repo stays private.
- CI publishes per-platform app zips and a signed `latest.json` manifest as release assets on `snappyref-dist`. The Semantic Search asset is a separate long-lived release asset on the same public distribution repo and is not republished for ordinary app releases.
- The app fetches `latest.json` directly from the GitHub release URL. No website cron, no API route.
- Each platform zip contains a single top-level `SnappyRef/` folder so the user's "drag the folder anywhere" model is uniform across macOS and Windows. On macOS the folder contains `SnappyRef.app`, including the macOS updater helper inside the app bundle. On Windows the folder contains only `snappyref.exe`.
- One zip flavor per platform: app-only zips (`snappyref-mac-{tag}.zip`, `snappyref-windows-{tag}.zip`) used for both first install and auto-update payloads. They exclude Semantic Search assets.

### Manifest

- `latest.json` schema includes `channel`, `version`, per-platform `{ url, sha256, size }`, release-notes URL, and a `signature` field over the canonical manifest bytes.
- Only `channel: "stable"` exists at runtime. The field is reserved for future beta/insider channels without requiring a schema migration.
- Manifest is signed with an ed25519 keypair. The public key is compiled into the app as a constant. The private key lives as a GitHub Actions secret used by the publish workflow.

### Client behavior

- The client checks for updates after **Settled Idle** following startup (never blocks startup), then every 6 hours while the app remains running.
- Discovery (fetching the manifest) is separate from download (fetching the zip). Discovery is automatic; download is gated on explicit user click.
- The client verifies the manifest signature before trusting any field. It verifies the downloaded zip's sha256 against the manifest before applying.
- A failed check (no network, GitHub unreachable, signature mismatch) backs off exponentially up to 6 hours and never surfaces a popup. Settings keeps the last known update state.
- The ping is a plain GET with no parameters, no body, no telemetry. The client filters per-platform fields locally.

### UI

- The Settings tab includes an `About / Updates` section with current version, last-checked time, manual `Check for updates`, update download/install progress, and a `Restart` action when a staged update is ready.
- A staged update discovered on a later launch stays restartable; it must not render as an in-progress finish state unless the user-initiated update flow is actively applying it.
- Rollback UI is deferred for v1.

### Install mechanics

- Downloaded zip is staged inside the install folder at `<install_root>/updates/staging-{version}/`, alongside `library/` and `models/`. This keeps all SnappyRef-related state inside the user's portable folder; no system app-data directories are used.
- On user confirmation: the app saves the **User Session** and stops worker child processes (embedding helper, image preparation helper).
- **macOS** writes a pending-update descriptor into `<install_root>/updates/`, launches the `snappyref-updater` helper binary from inside `SnappyRef.app`, and exits. The helper waits for the parent process to exit, renames current `SnappyRef.app` to `SnappyRef.app.previous`, moves the staged bundle into place at `<install_root>/SnappyRef.app`, launches it with `open`, removes staging, and exits.
- **Windows** uses a single-exe self-replace path. The running app writes the pending-update descriptor, copies its current executable to a temporary runner under `<install_root>/updates/`, launches that runner, and exits. The runner copies the staged `snappyref.exe` to `snappyref.exe.new`, flushes it, waits until the live executable can be renamed, renames the current executable to `snappyref.exe.old`, renames `.new` into the live `snappyref.exe` slot, launches the new executable with the original launch args, removes staging, and exits. The temporary runner is best-effort cleaned by normal update-file cleanup on a later launch.
- The macOS helper explicitly strips the `com.apple.quarantine` xattr from the staged bundle before swap as defense-in-depth. In-process `reqwest` downloads do not set the xattr, so Gatekeeper does not engage on the swapped bundle. Real Developer ID signing remains a future, separate decision.
- After a successful launch of the new version, the staging directory `<install_root>/updates/staging-{version}/` is removed; the `.previous` / `.old` backup is retained until the next update overwrites it.

### Windows single-exe update path

Windows does not ship a permanent updater helper. Earlier designs used `SnappyRef/snappyref-updater.exe`, but that introduced helper updateability, UAC filename heuristics, and stale-helper failure modes while the Windows release unit is only one executable. Windows cannot reliably overwrite the live executable from the process whose image is mapped, so SnappyRef copies the current exe to a temporary runner and lets that separate process perform the rename-based self-replace flow after the app exits.

This keeps Windows package shape and update behavior local to `snappyref.exe`. If Windows later gains required sidecar files, this decision must be revisited before those sidecars become part of the release contract.

### Rollback

- Exactly one prior version is retained as `SnappyRef.app.previous` / `snappyref.exe.old` so failed update investigations have a local recovery source. Each update overwrites the prior backup.
- User-facing rollback is not supported in v1. No Settings rollback button and no automatic watchdog rollback.

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
- Update staging lives under `<install_root>/updates/`. If the user has placed `SnappyRef.app` directly into `/Applications` on macOS, the install root is admin-owned and the updater cannot write `updates/` or swap the bundle. The website download page should instruct users to keep the SnappyRef folder in a writable location. A future enhancement may detect protected app installs and surface a clear error.
- Discovery is opportunistic; there is no guarantee of immediate update propagation. A critical fix may take up to 6 hours per user to surface. Acceptable for the user base v1 targets; force-update can be revisited later.
- No telemetry is collected. Adoption metrics are not available; this is intentional for v1 and can be reconsidered with explicit consent later.
- ADR-0006 (manual updates for v1) is superseded by this decision.
