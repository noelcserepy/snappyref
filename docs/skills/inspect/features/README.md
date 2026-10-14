# SnappyRef verification map

This directory lists SnappyRef's user-facing features and how to prove each one
with inspect scenarios. Read this index, then follow the matching feature file.

## Running and reading evidence

Follow [Start Here](../SKILL.md#start-here) for checkout checks and scenario
commands, [Isolation and cleanup](../SKILL.md#isolation-and-cleanup) for data
roots, and [Evidence Rules](../SKILL.md#evidence-rules) and
[Reporting Back](../SKILL.md#reporting-back) for artifacts and reports.
The [real-app MCP route](../SKILL.md#driving-the-real-app-through-mcp) has
separate coverage from harness input. [AI control](./ai-control.md) lists it.

The harness reports the OS as Linux. Scenario modifiers send Ctrl where a
macOS user presses ⌘. The delete key follows the build target, so it is
Backspace on macOS and Delete on Windows.

Each feature file marks which steps a scenario seeds and which it drives.
A step counts as covered only when a `Driving it with inspect` bullet names
it as driven. Seeded steps prove nothing about their entry points. Report
uncovered entry points as unverified. For persistence, cite an assertion that
reads the library back after a flush.

Native clipboard reads, Finder and browser drops, PureRef paste, and Windows
behavior need native checks. Use [use-windows](../../use-windows/SKILL.md)
for Windows. When no scenario drives the changed entry point, follow
[Writing A New Scenario](../SKILL.md#writing-a-new-scenario).
The [AGENTS.md explicit-request rule](../../../../AGENTS.md#donts) governs
networked install and update verification.

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
- [Annotations](./annotations.md): labels, arrows, lines, rectangles, styles,
  and endpoint drags.
- [Semantic Search](./semantic-search.md): install, the palette, image search,
  and placing results.
- [PureRef exchange](./pureref-exchange.md): `.pur` drop import and ⇧⌘E
  export.
- [AI control](./ai-control.md): the access setting, MCP tool calls, the
  background launch, and the confirmation an AI client waits on.
