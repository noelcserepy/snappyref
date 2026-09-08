# Release Operations

SnappyRef releases are app-only updates published through `snappyref-dist`.
Semantic Search assets are distributed separately and are not bundled into
ordinary app release zips.

## Release Model

- `snappyref-dist` hosts the public release assets: per-platform app zips,
  signed `latest.json`, and the standalone `snappyref-semantic-search.zip`.
- App release zips contain a single top-level `SnappyRef/` folder.
- macOS zips contain `SnappyRef/SnappyRef.app/`, including the macOS
  `snappyref-updater` helper inside the app bundle.
- Windows zips contain `SnappyRef/snappyref.exe` only.
- Release zips must not contain `library/`, `models/`, `UPDATE.txt`, or any
  user-owned data.
- The **Local Model Store** is acquired separately by Semantic Search install
  flows and survives app updates.

## Signing And Secrets

- `MANIFEST_SIGNING_KEY`: ed25519 private key used by
  `publish-full-release.yml` to sign `latest.json`.
- `DIST_REPO_TOKEN`: fine-grained token with Contents read/write access to
  `noelcserepy/snappyref-dist`.
- Public key: [keys/signing-key.pub.pem](keys/signing-key.pub.pem) is the single
  source for release verification and the app's trusted key. Cargo decodes and
  validates it during the build, then embeds its 32 key bytes. The installed app
  does not read a separate key file. Invalid PEM or a different key algorithm
  fails the build.

If the private key is leaked, generate a new keypair, replace the GitHub secret
and the PEM file, then rebuild and ship a manual app update embedding the new
public key. No Rust constant needs updating. Old clients reject manifests signed
by a key they do not know.

## Publish Full Release

1. Make sure `main` contains the intended release commit.
2. Run the local pre-push gate or let the push hook run it:
   `cargo fmt --check`, shipping-feature `cargo check`, and
   `update_preservation_smoke`.
3. Bump the package version and release workflow defaults as needed.
4. Dispatch `Publish Full Release` with the release tag.
5. Keep `build_profile=release` for public builds.
6. Inspect the draft `snappyref-dist` release assets before publishing.
7. Confirm archive validation passed for both app zips.
8. Run update smokes when the release touches update, startup, storage,
   packaging, or platform-specific behavior.

## Publish Semantic Search Asset

Run `Publish Semantic Search` only when the local search assets change or need
to be republished. Ordinary app releases reuse the existing standalone asset.
The app build embeds the Semantic Search descriptor values so install surfaces
can verify URL, size, and sha256 before promoting the **Local Model Store**.

## Update Smokes

Use these scripts only when explicitly verifying the published update channel:

```sh
scripts/update-smoke-macos.sh
scripts/update-smoke-windows.sh
```

Both scripts build a scratch install, force the running app to report an old
version, fetch the published `latest.json`, download and verify the latest
platform release, stage it, and trigger the normal update apply path.

- macOS verifies the bundled helper swap path and retained
  `SnappyRef.app.previous` backup.
- Windows verifies the single-exe self-replace path and retained
  `snappyref.exe.old` backup.

These scripts are networked, launch the GUI app, depend on `snappyref-dist`, and
are not pre-push checks.

## Install And Data Root Contract

SnappyRef prefers a writable portable data root beside the installed app. If the
install root is protected, the app falls back to the user's app-data directory.

- `SNAPPYREF_DATA_ROOT` overrides the shared data root for the **Library**,
  **Local Model Store**, and Semantic Search staging.
- `SNAPPYREF_LIBRARY_ROOT` overrides only the **Library** path for tests and
  diagnostics.
- Release zips never include or remove `library/`; the Library is user-owned.
- Downgrade is unsupported after a newer app has opened and migrated user data.
