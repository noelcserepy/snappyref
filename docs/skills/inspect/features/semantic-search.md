# Semantic Search

Users find images in their library by describing them, or by searching with a
selected image. The search model is a one-time 253 MB download that the app
offers at first launch, from Settings, and from the search palette. Results can
be placed with Enter or dragged onto the canvas.

## Sub-features

- `search-first-launch` is the "Enable Semantic Search?" dialog with "Not now"
  and "Enable". Enter enables it and Escape dismisses it.
- `search-settings` is the Settings → General "SEMANTIC SEARCH" row. It shows
  "Install", "Cancel", "Retry", or "Remove". "Remove" asks for confirmation
  with "Remove Semantic Search?".
- `search-install-card` is the palette card. It shows "Install Semantic
  Search", "Install", and "Not now", then a progress bar with "Cancel", then
  "Install failed" with "Retry". The card works by mouse only.
- `search-palette` opens with ⌘F / Ctrl+F. It has a "Search images" input, a
  "Recent" list, results, and "No results". Arrow keys select. ↵ picks up the
  selected results and ⌘↵ picks up all of them, as a provisional batch at the
  pointer. A click commits the batch and Esc cancels it.
- `search-image` uses ⌘F with one image selected, or the "Search using image"
  menu row. It shows a `similar to #{id}` pill.
- `search-drag-place` drags results onto the canvas as provisional placements.
  Releasing commits them, and Esc or a right click cancels.

## How to get to it (user POV)

- Launch with a library and no model. The first-launch dialog appears.
- Press ⌘F / Ctrl+F, or right-click an image and choose "Search using image".
- Open the sidebar, go to "Settings" → "General", and find the "SEMANTIC
  SEARCH" row.
- An AI client calls `search_library` and `search_model`. See
  [AI control](./ai-control.md).

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.
- Scenarios inject model status and results, except `semantic-search-real-install`
  and `image-search-after-install`. The injected scenarios do not rank, index,
  or download.

- **First launch and remove dialog.** Run `run-scenario.sh semantic-search-install`.
  It checks that ⌘F stays blocked behind the prompt and that Enter starts the
  install with a test installer. "first-launch prompt is marked seen" reads the
  library. The Remove dialog is opened in code, and only its Escape is driven.
- **Palette states.** Run `run-scenario.sh search-composer`. Frames
  `screen-000-recents` through `screen-006-install-error` cover recents,
  results, no results, the image pivot, the install card, progress, and an
  error. Enter and "Not now" are driven on the install card. "result previews
  decoded before capture" proves the thumbnails were visible.
- **Image search right after install.** Run
  `run-scenario.sh image-search-after-install`. It drives a click on an
  unindexed image, ⌘F, and "Install" on the palette card. The real service then
  embeds the image with an offline helper,
  `EmbeddingHelperLaunch::Fixture`. That helper loads in 400 ms and takes 150 ms
  per image. The scenario asserts that the footer says "Indexing…", never
  "Search error", and that the ranked results then appear on their own.
- **Image search to canvas.** Run `run-scenario.sh blue-image-search-drag`. It
  drives a click and two ⌘-clicks to select 3 results, then a drag and drop.
  The image search itself is opened in code, not with ⌘F or the menu.
- **Result drag basics.** Run `run-scenario.sh search-drag-to-canvas`, which
  `check-harness.sh` also runs. It renders the full app before and after the
  drop and checks the source asset and placement at the drop pointer.
- **Keyboard placement.** Run `run-scenario.sh search-keyboard-placement`.
  An installed palette and six results are seeded without starting a model.
  Right, Shift-Right, Down, Up, Enter, Escape, and Mod-Enter are driven.
  The scenario checks exact asset identities, literal drop centers, durable
  placements, one-batch Undo, and Redo after cancellation. Recent queries are
  restored by arrows/Enter and a click, with saved order read from the library.
  This proves query restoration, not a model search result.
- **Settings row.** Run `run-scenario.sh semantic-search-sidebar-status`. It
  injects a download in progress and asserts "Downloading 42%" with Cancel.
  Its output does not depend on this machine's model files.
- **Settings install.** Run `run-scenario.sh semantic-search-stubbed-install`.
  It drives the same clicks as `semantic-search-real-install` through one
  shared function, with a stub installer that reports Installed at
  once. `cargo test` runs it, so a Settings change that breaks the
  real-install selectors fails a normal test.

## Gotchas

- Only `image-search-after-install` opens image search with ⌘F. The
  "Search using image" menu row has no scenario.
- `search-keyboard-placement` drives arrow selection, ↵ and ⌘↵ placement,
  and Escape cancellation. Right-click cancellation and pagination past the
  final loaded row remain manual.
- Only `image-search-after-install` clicks "Install" on the palette card. No
  scenario clicks the card's "Cancel" or "Retry", or "Cancel", "Retry", or the
  confirmed "Remove" in Settings. Plain tests in
  `src/app/tests/semantic_search.rs` cover some of these, without artifacts.
- Real ranking and the real download need the real model. The fixture helper
  embeds every image as the same vector, so only seeded embeddings rank. Never
  run `semantic-search-real-install` unless the user asks.
- `mcp-drive.py` runs on a root with no model, so `search_library` and
  `search_model` are only seen being refused.
- Result count does not prove the thumbnails are visible. Only
  `search-composer` waits for preview decoding.
- Enter enables from the first-launch dialog. Enter does nothing on the install
  card or the Remove dialog.
- ⌘F does nothing while any modal is open, including the first-launch dialog.
