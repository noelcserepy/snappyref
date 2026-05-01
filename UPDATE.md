# Update Path

Reference should use a simple portable update model. User data lives next to the
app and updates replace only app-owned files.

## Install Folder Contract

The app folder should look like this:

```text
Reference/
  reference.exe
  models/
  library/
    library.db
    originals/
    thumbs/
```

`library/` is user-owned data. Release archives must not include it, delete it,
or replace it.

`reference.exe` and `models/` are app-owned release files. They may be replaced
when updating.

## Update UX For V1

Use a manual update flow:

1. User quits Reference.
2. User downloads the new release zip from the website or GitHub release page.
3. User extracts the zip into the existing `Reference/` folder.
4. User replaces `reference.exe` and `models/` when prompted.
5. User keeps the existing `library/` folder in place.
6. User launches the new app and continues from the same session state.

Do not build in-app self-update for v1. The app may later add a lightweight
"Update available" button that opens the download page, but it should not
download, patch, replace, or restart the app automatically.

## Required App Behavior

- Resolve the install root from `std::env::current_exe().parent()`, not the
  process current working directory.
- Resolve the library root as `install_root/library`.
- Keep an environment override such as `REFERENCE_LIBRARY_ROOT` for tests,
  debug probes, and local development.
- Create `library/` on first launch if it is missing.
- On later launches, open the existing `library/` and run only forward schema
  migrations.
- Never delete or recreate an existing `library/` as part of startup.

## Release Artifact Shape

Release zips should contain only app-owned files:

```text
reference-windows-v0.2.0.zip
  reference.exe
  models/
  UPDATE.txt
```

The zip should not contain a top-level `library/` directory.

`UPDATE.txt` should include short user-facing instructions:

```text
To update:
1. Quit Reference.
2. Extract this zip into your existing Reference folder.
3. Replace reference.exe and models/ when prompted.
4. Do not delete or replace the library/ folder.
```

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
