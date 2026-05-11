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

## Update UX

Since ADR-0015, SnappyRef performs **in-app automatic updates**. The app polls a
signed manifest in the background, surfaces an Available Update through a
sidebar pill, and on user confirmation downloads, verifies, swaps, and
relaunches via the `snappyref-updater` helper binary. See
`docs/adr/0015-automatic-app-updates.md` for the full flow.

Manual zip-replace (the original v1 plan documented in ADR-0006) still works as
a fallback for users who download a release zip directly from GitHub or the
website — the install-folder contract below is the same either way. Auto-update
only runs against installs sitting in a writable location.

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
   reusable model bundle is missing. Leave `save_as_reusable_model_bundle`
   enabled so future packaging runs can reuse it.
2. Run `Build Windows App` or `Build macOS App` for the app commit you want to
   test or ship. These build app-only artifacts:
   - Windows: `SnappyRef/snappyref.exe` and `SnappyRef/UPDATE.txt`
   - macOS: `SnappyRef.app` and `UPDATE.txt`, without bundled models
3. Run `Package Release Zips`. Choose Windows, macOS, or both. Leave app run
   ids blank to use the latest successful app builds, or paste explicit run ids
   when you need exact inputs. Set `include_models=true` for full install zips
   or `include_models=false` for a smaller Windows update zip.

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

### Full Release Workflow

When `main` is ready for the next version, publish update zips through
`Publish Full Release`:

1. Switch to `main` and pull the latest changes.
2. Run the release gate locally by invoking the pre-push hook directly:
   `./.githooks/pre-push`. Same three checks (fmt + `cargo check` under shipping
   features + the `update_preservation_smoke` test) that fire on every push.
3. Bump the package version if this is a real release.
4. Commit and push the version bump.
5. Run `Publish Full Release` with `release_tag`, for example `v0.1.0`.
6. Keep `build_profile=release` for public builds.
7. Leave `create_draft_release=true` so the final zips attach to a draft
   GitHub Release that can be inspected before publishing.
8. Download the release zips and run the real-zip update smoke checklist before
   broadly publishing the release.

For a fast test app build, run `Build Windows App` or `Build macOS App` with
`build_profile=release-fast`. That profile skips native turbojpeg and full
release optimization, so it should not be used for public releases.

GitHub Actions artifacts are always downloaded as GitHub-generated zip wrappers.
For direct one-zip downloads, use zips attached to a GitHub Release by
`Package Release Zips` or `Publish Full Release`.

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
