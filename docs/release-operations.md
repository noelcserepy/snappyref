# Release Operations

SnappyRef releases are app-only updates published as this repo's GitHub
Releases. Semantic Search assets are distributed separately and are not bundled
into ordinary app release zips.

## Release Model

- Each app release here carries the per-platform app zips and the signed
  `latest.json`. The app reads `releases/latest/download/latest.json`, which
  follows the newest published release that is not a prerelease.
- The standalone `snappyref-semantic-search.zip` lives on the `semantic-search`
  prerelease, which never takes the `latest` alias.
- `snappyref-dist` is the old release host, now archived. See
  [Older apps](#older-apps).
- App release zips contain a single top-level `SnappyRef/` folder.
- macOS zips contain `SnappyRef/SnappyRef.app/`, including the macOS
  `snappyref-updater` helper and `Contents/Resources/THIRD_PARTY_NOTICES.txt`
  inside the app bundle.
- Windows zips contain `SnappyRef/snappyref.exe` and
  `SnappyRef/THIRD_PARTY_NOTICES.txt`. The update swap replaces only the exe,
  so an updated install keeps the notices file from its first install.
- `scripts/third_party_notices.py` writes the notices during packaging from
  `cargo metadata --locked`, so they follow `Cargo.lock` without a regeneration
  step. It fails when a crate ships no license file and offers no license in
  `assets/licenses/`, or when ort-sys links an ONNX Runtime version other than
  the vendored `assets/licenses/onnxruntime-<version>/`.
- Release zips must not contain `library/`, `models/`, `UPDATE.txt`, or any
  user-owned data.
- The **Local Model Store** is acquired separately by Semantic Search install
  flows and survives app updates.

## Signing And Secrets

- `SNAPPYREF_WEB_RELEASE_REFRESH_URL` and
  `SNAPPYREF_WEB_RELEASE_REFRESH_SECRET`: optional website refresh webhook and
  its `x-snappyref-release-refresh-secret` header value. The full-release
  workflow skips refresh if either secret is absent. Its refresh runs before
  draft publication; the release skill owns the post-publication refresh.
- `MANIFEST_SIGNING_KEY`: ed25519 private key used by
  `publish-full-release.yml` to sign `latest.json`.
- Public key: [keys/signing-key.pub.pem](keys/signing-key.pub.pem) is the single
  source for release verification and the app's trusted key. Cargo decodes and
  validates it during the build, then embeds its 32 key bytes. The installed app
  does not read a separate key file. Invalid PEM or a different key algorithm
  fails the build.

If the private key is leaked, generate a new keypair, replace the GitHub secret
and the PEM file, then rebuild and ship a manual app update embedding the new
public key. No Rust constant needs updating. Old clients reject manifests signed
by a key they do not know.

## App release procedure

Use the [release skill](skills/release/SKILL.md) for the version bump PR,
patchnotes, dry run, draft inspection, publication and public-channel checks.
The workflow refuses published tags and keeps signed draft zips and their
manifest together; see [Reuse app builds](#reuse-app-builds).

## Older apps

Apps from v0.3.0 and older check
`snappyref-dist/releases/latest/download/latest.json` for updates and download
the Semantic Search model from dist's `semantic-search` release. The v0.3.1
release published a copy of its signed `latest.json` there, with zip URLs
pointing here, so an older app updates to v0.3.1 once and reads this repo's
manifest from then on. `snappyref-dist` is archived and must stay online, not
deleted, for installs that have not updated yet.

## Publish Semantic Search Asset

The [model-packaging skill](skills/model-packaging/SKILL.md) owns building and
publishing the model package and updating the descriptor. The workflow uploads
to this repo's `semantic-search` prerelease. Ordinary app releases reuse the
existing standalone asset.
[assets/semantic-search-descriptor.env](../assets/semantic-search-descriptor.env)
is the canonical URL, SHA256 and size. Cargo and both build workflows read it.
Workflow inputs and `SNAPPYREF_SEMANTIC_SEARCH_*` build variables override values
when needed.
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
Blank build tags use the target `release_tag`. Every successful Package Release Zips
run attaches its final zips to that release, creating a draft if the tag has no release.
Both workflows upload through `scripts/upload-release-assets.sh`, which keeps a
release's zips and signed `latest.json` matching. It refuses a published release,
and it refuses new zips on a draft that holds a signed `latest.json` unless a new
`latest.json` comes with them. Full Release uses the same packaging action,
attaches the zips to its draft, signs its manifest, and removes its temporary build
handoff assets.

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

These scripts are networked, launch the GUI app, depend on the published
release, and are not pre-push checks.

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
