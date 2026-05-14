# Release Update Smoke Checklist

Use this checklist before the first public release and for any release that can touch storage,
startup routing, bundled assets, or app migration code.

## Published-Manifest Smokes

Run these only when you explicitly need to prove the real update channel works:

```sh
scripts/update-smoke-macos.sh
scripts/update-smoke-windows.sh
```

The macOS script builds a local app bundle into `target/update-smoke/macos/SnappyRef`.
The Windows script syncs the Mac checkout to the configured Windows machine, builds
there, and creates `target\update-smoke\windows\SnappyRef` on that machine.

Both launch the scratch app with `SNAPPYREF_UPDATE_CURRENT_VERSION=0.0.0`, fetch
the published GitHub `latest.json`, download and verify the latest release zip for
the host platform, trigger the normal `snappyref-updater` helper, and check that
the scratch install was swapped with a `.previous` / `.old` backup retained.

These are not pre-push checks. They are networked, depend on `snappyref-dist`, launch
the GUI app, and intentionally test the public channel rather than a local fixture.

## First-Release Update Drill

This drill is mandatory before the first release. It must use an actual generated release zip,
not a local debug build, copied binary, or hand-assembled folder.

1. Generate the release artifact using the release packaging workflow.
2. Create a fake portable install directory named `SnappyRef/` in a temporary location.
3. Extract the current release zip into that fake portable `SnappyRef/` install.
4. Launch the app from that install before update.
5. In the launched app, create representative user state:
   - a populated `library/` with at least one board and imported image data
   - board layout state that is visually recognizable after restart
   - session state such as the active board, viewport, selection, search, or sidebar state
6. Quit the app and confirm the fake portable install now contains `library/`.
7. Generate or obtain the new release zip that is being tested.
8. Extract the new zip over the existing fake portable `SnappyRef/` install.
9. Verify `library/` remains present after extraction and was not replaced, removed, or moved.
10. Launch the app after update from the same fake portable install.
11. Confirm the same library data, active board, board layout, and session state are still present.
12. Exercise one basic interaction that reads user data, such as opening the active board,
    selecting an existing item, or running search against the updated install.
13. Record the release zip names, app versions, operating system, and whether the update drill passed.

## Downgrade Policy

Downgrade is unsupported after forward migration. Once an updated app has opened and migrated a
portable `SnappyRef/` install, do not use older app versions against that same install unless a
specific rollback path has been designed and tested for that migration.

## Pass Criteria

The update smoke passes only if:

- the test used an actual generated release zip for both the baseline install and update
- `library/` stayed in place across extraction
- launch before update created representative library and session state
- launch after update preserved the same board, data, and session state
- any forward migration behavior and downgrade risk were noted in the release record
