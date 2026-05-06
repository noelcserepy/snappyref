# Update Path

SnappyRef should use a simple portable update model. User data lives next to the
app and updates replace only app-owned files.

## Install Folder Contract

The app folder should look like this:

```text
SnappyRef/
  snappyref.exe
  models/
  library/
    library.db
    originals/
    thumbs/
```

`library/` is user-owned data. Release archives must not include it, delete it,
or replace it.

`snappyref.exe` and `models/` are app-owned release files. Windows updates
normally replace only `snappyref.exe`; `models/` ships with releases for fresh
installs and only needs replacement when the model bundle changes.

## Update UX For V1

Use a manual update flow:

1. User quits SnappyRef.
2. User downloads the new release zip from the website or GitHub release page.
3. User extracts the zip.
4. On Windows, user copies the new `SnappyRef/snappyref.exe` over the old `snappyref.exe`.
5. User keeps the existing `library/` folder in place.
6. User launches the new app and continues from the same session state.

Do not build in-app self-update for v1. The app may later add a lightweight
"Update available" button that opens the download page, but it should not
download, patch, replace, or restart the app automatically.

## Required App Behavior

- Resolve the install root from `std::env::current_exe().parent()`, not the
  process current working directory.
- Resolve the library root as `install_root/library`.
- Keep an environment override such as `SNAPPYREF_LIBRARY_ROOT` for tests,
  debug probes, and local development.
- Create `library/` on first launch if it is missing.
- On later launches, open the existing `library/` and run only forward schema
  migrations.
- Never delete or recreate an existing `library/` as part of startup.

## Release Artifact Shape

Release zips should contain only app-owned files:

```text
snappyref-windows-v0.2.0.zip
  SnappyRef/
    snappyref.exe
    models/
    UPDATE.txt
```

The zip should not contain `library/` user data.

`UPDATE.txt` should include short user-facing instructions:

```text
To update:
1. Quit SnappyRef.
2. Extract this zip.
3. On Windows, open the extracted SnappyRef folder and replace snappyref.exe.
4. Do not delete or replace the library/ folder.
```

## Publishing A Manual Update Zip

### Modular Packaging Workflow

Use the split workflows when app builds and model packaging should be decoupled:

1. Run `Build Model Bundle` when the semantic-search model changes or when the
   reusable model release asset is missing. This publishes
   `snappyref-models-siglip2_base_patch16_224_int8.zip` to the configured model
   release tag.
2. Run `Build Apps` for the app commit you want to test or ship. This builds
   app-only artifacts:
   - Windows: `SnappyRef/snappyref.exe` and `SnappyRef/UPDATE.txt`
   - macOS: `SnappyRef.app` and `UPDATE.txt`, without bundled models
3. Run `Package Release Assets` with the `Build Apps` run id. Set
   `include_models=true` for full install zips or `include_models=false` for a
   smaller Windows update zip.

The packaged Windows full zip has this shape:

```text
snappyref-windows-full.zip
  SnappyRef/
    snappyref.exe
    UPDATE.txt
    models/
```

The packaged Windows update zip omits `models/`:

```text
snappyref-windows-update.zip
  SnappyRef/
    snappyref.exe
    UPDATE.txt
```

macOS keeps models inside `SnappyRef.app/Contents/Resources/models`. Because of
that, the modular packaging workflow only creates a macOS zip when
`include_models=true`; replacing a model-less `.app` would remove the bundled
models.

For a direct one-zip download, publish the packaged assets as GitHub Release
assets. GitHub Actions artifacts are always downloaded as GitHub-generated zip
wrappers and should be treated as temporary handoff artifacts between
workflows.

### Tagged Release Workflow

When `main` is ready for the next version, publish update zips through the
tagged release workflow:

1. Switch to `main` and pull the latest changes.
2. Run the release gate locally:
   - `cargo fmt --check`
   - `cargo check --locked --no-default-features --features semantic-search`
   - `cargo test --locked --no-default-features update_preservation_smoke`
3. Bump the package version if this is a real release.
4. Commit and push the version bump.
5. Create and push a version tag, for example `v0.2.0`.
6. Let the GitHub Actions release workflow build and upload the Windows and
   macOS zips.
7. Download the release zips and run the real-zip update smoke checklist before
   broadly publishing the release.

For a fast test build, run the manual `Build Artifacts` workflow from GitHub
Actions and set `ref` to `main`, a branch, or a commit SHA. The default
`release-fast` profile skips native turbojpeg and full release optimization so
Windows test artifacts do not pay the full release build cost.

GitHub Actions artifacts are always downloaded as a GitHub-generated zip. For a
direct one-zip test download, enable `publish_test_release` on the manual
workflow. It creates a draft prerelease with normal release zip assets.

## Verification

Add a release-preservation smoke test before treating this path as supported:

1. Create a fake install folder with app-owned files and a seeded `library/`.
2. Save representative user data:
   - imported original image
   - board item
   - active board
   - viewport
   - workspace settings
   - sidebar state
3. Simulate an update by replacing only app-owned files.
4. Launch the new build against the same install folder.
5. Assert the new app loads the same library data and session state.

Database migrations are forward-only. Older app versions are not guaranteed to
read a library after a newer version has migrated it.
