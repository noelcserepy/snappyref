# Release Operations

SnappyRef releases are app-only updates published through `snappyref-dist`.
Semantic Search assets are distributed separately and are not bundled into
ordinary app release zips.

## Release Model

- `snappyref-dist` hosts the public release assets: per-platform app zips,
  signed `latest.json`, and the standalone `snappyref-semantic-search.zip`.
- App release zips contain a single top-level `SnappyRef/` folder.
- macOS zips contain `SnappyRef/SnappyRef.app/`, including the macOS
  `snappyref-updater` helper and `Contents/Resources/THIRD_PARTY_NOTICES.txt`
  inside the app bundle.
- Windows zips contain `SnappyRef/snappyref.exe` and
  `SnappyRef/THIRD_PARTY_NOTICES.txt`. The update swap replaces only the exe,
  so an updated install keeps the notices file from its first install.
- `scripts/third_party_notices.py` writes the notices during packaging from
  `cargo metadata --locked`, so they follow `Cargo.lock` without a regeneration
  step. It needs network access for the ONNX Runtime notices and fails when a
  crate ships no license file and offers no license in `assets/licenses/`.
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
2. Use the [local pre-push gate](../README.md#development-checks).
3. Bump the package version as needed. The release tag must match it.
4. Dispatch `Publish Full Release` with the release tag.
5. Public builds use the shipping `release` profile.
6. Inspect the draft `snappyref-dist` release assets before publishing.
7. Confirm archive validation passed for both app zips.
8. Run [update smokes](#update-smokes) only on explicit user request.

The [functional test catalog](release-functional-test-plan.md) covers the app's
user interactions on macOS and Windows. Follow the
[budgeted execution plan](release-test-execution.md) to combine existing code
tests with bounded native-agent workflows. Both exclude app updating.

## Publish Semantic Search Asset

Run `Publish Semantic Search` only when the local search assets change or need
to be republished. Ordinary app releases reuse the existing standalone asset.
[assets/semantic-search-descriptor.env](../assets/semantic-search-descriptor.env)
is the canonical URL, SHA256 and size. Cargo and both build workflows read it.
Workflow inputs and `SNAPPYREF_SEMANTIC_SEARCH_*` build variables override values
when needed. Update the descriptor after publishing a replacement model package.
`MODEL_BUNDLE.txt` records the model key and hashes of the tokenizer and both
ONNX files for manual package provenance. Keep it in model packages.
`scripts/semantic_model.py` downloads a pinned Hugging Face revision and adds
the Apache-2.0 `LICENSE` and a `NOTICE` next to the model files. The app's
notices carry the same text because the published package predates both.
Build workflows attach the resolved descriptor alongside the app archive.
Packaging rejects invalid descriptors or differing descriptors across selected builds.

## Reuse app builds

Build Windows App and Build macOS App attach their app archive and descriptor to
`handoff_release_tag` in a source-repo draft release. Package Release Zips accepts
`windows_build_tag` and `macos_build_tag` to reuse those builds without compiling.
Blank build tags use the target `release_tag`. Every successful packaging run
attaches its final zips to that release, creating a draft if the tag has no release.
Existing releases keep their publication state. Full Release uses the same packaging action,
then signs its manifest and removes its temporary build handoff assets.

Local packaging verification needs no release or model download:

```sh
python3 -m unittest discover -s scripts -p test_release_packaging.py
```

The icon source is `assets/app-icon/snappyref.svg`. On macOS, install Pillow and
CairoSVG in a Python environment, then run `python scripts/generate-app-icons.py`.
It renders the SVG to the checked-in runtime PNG, Windows ICO and macOS ICNS.
Conversion tools are not installed with the app.

## Update Smokes

Run these scripts only on explicit user request to verify the published update
channel. Release scope alone does not authorize them:

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
  **Local Model Store**, workspace session state, Asset embeddings, and Semantic
  Search staging.
- `SNAPPYREF_LIBRARY_ROOT` overrides Library content, workspace session state,
  and Asset embeddings for tests and diagnostics. Model files and Semantic
  Search staging still follow the data root.
- `SNAPPYREF_INSTALL_ROOT` overrides the install root for App Update
  development and verification only. It does not relocate Library or model data.
- Release zips never include or remove `library/`; the Library is user-owned.
- Downgrade is unsupported after a newer app has opened and migrated user data.
