# SnappyRef verification map

This directory lists SnappyRef's user-facing features and how to prove each one
with inspect scenarios. Read this index, then follow the matching feature file.

## Baseline preconditions

- `docs/skills/inspect/scripts/doctor.sh` exits 0 in the checkout under test.
- `CARGO_TARGET_DIR` is unset.
- Each scenario seeds its own temp library. No shared app state needs to be
  restored between runs.

## Driving conventions

- Run scenarios with `docs/skills/inspect/scripts/run-scenario.sh <scenario>`.
  `status=pass` and an `artifact=` path mean that run produced a complete
  report. A pass says nothing about other runs or other scenarios.
- The harness reports the OS as Linux. Scenario modifiers send Ctrl where a
  macOS user presses ⌘. The delete key follows the build target, so it is
  Backspace on macOS and Delete on Windows.
- Each feature file marks which steps a scenario seeds in code and which it
  drives as a user would. A seeded step proves nothing about its own entry
  point.
- When no scenario drives the entry point you changed, write one. "Writing A
  New Scenario" in [../SKILL.md](../SKILL.md) covers how. Do not launch the GUI
  by hand.
- Native OS paths need a real platform check, and in-process scenarios cannot
  reach them. These are clipboard reads, Finder and browser drops, PureRef
  paste, and the Windows build. Use [use-windows](../../use-windows/SKILL.md)
  for Windows.
- Never run `semantic-search-real-install` unless the user asks. It is
  networked.

## Proof and skip reporting

- Report the scenario command, the artifact path, and the assertions that cover
  the change.
- Read `report.json`, then `events.ndjson`, then `frames/*.png`. Open the
  frames that show the action and the state after it, not only the last frame.
- For a persistence change, cite an assertion that reads the library back after
  a flush. Each feature file names the scenarios that do this.
- Report each unscenarioed or native entry point as unverified, and name it. A
  different entry point passing does not cover it.

## Feature entry contract

Each feature file has an H1 and a one-paragraph summary, then these H2s in
order: `Sub-features`, `How to get to it (user POV)`,
`Driving it with inspect`, `Gotchas`. Shortcuts are written macOS first, then
Windows.

## Features

- [Image import](./image-import.md): file drop, paste, URL drag, and loading
  batches.
- [Canvas editing](./canvas-editing.md): select, move, scale, rotate, crop,
  arrange, delete, undo/redo, and the context menu.
- [Boards and navigation](./boards.md): create, rename, switch, delete, pan,
  zoom, the sidebar, and session restore.
- [Groups](./groups.md): group, ungroup, the picker, the group dock, and the
  member dock.
- [Annotations](./annotations.md): arrows, lines, textboxes, styles, and
  endpoint drags.
- [Semantic Search](./semantic-search.md): install, the palette, image search,
  and placing results.
