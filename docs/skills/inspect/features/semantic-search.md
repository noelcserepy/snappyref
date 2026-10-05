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
  "Recent" list, results, and "No results". Arrow keys select, ↵ places, and
  ⌘↵ places all.
- `search-image` uses ⌘F with one image selected, or the "Search using image"
  menu row. It shows a `similar to #{id}` pill.
- `search-drag-place` drags results onto the canvas as provisional placements.
  Releasing commits them, and Esc or a right click cancels.

## How to get to it (user POV)

- Launch with a library and no model. The first-launch dialog appears.
- Press ⌘F / Ctrl+F, or right-click an image and choose "Search using image".
- Open the sidebar, go to "Settings" → "General", and find the "SEMANTIC
  SEARCH" row.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.
- Every scenario except `semantic-search-real-install` injects model status and
  results. None of them ranks, indexes, or downloads.

- **First launch and remove dialog.** Run `run-scenario.sh semantic-search-install`.
  It checks that ⌘F stays blocked behind the prompt and that Enter starts the
  install with a test installer. "first-launch prompt is marked seen" reads the
  library. The Remove dialog is opened in code, and only its Escape is driven.
- **Palette states.** Run `run-scenario.sh search-composer`. Frames
  `screen-000-recents` through `screen-006-install-error` cover recents,
  results, no results, the image pivot, the install card, progress, and an
  error. Enter and "Not now" are driven on the install card. "result previews
  decoded before capture" proves the thumbnails were visible.
- **Image search to canvas.** Run `run-scenario.sh blue-image-search-drag`. It
  drives a click and a ⌘-click to select 3 results, then a drag and drop. The
  image search itself is opened in code, not with ⌘F or the menu.
- **Result drag basics.** Run `run-scenario.sh search-drag-to-canvas`, which
  `check-harness.sh` also runs. It renders only the overlay. Its one frame is
  a drawn rectangle and not a screenshot.
- **Settings row.** Run `run-scenario.sh semantic-search-sidebar-status`. It
  shows whatever model status this machine has on disk, and no assertion checks
  the status text.

## Gotchas

- Opening search with ⌘F or "Search using image" has no scenario.
- No scenario drives result selection with the arrow keys, ↵ or ⌘↵
  placement, or cancelling a result drag with Esc or a right click.
- No scenario clicks "Install", "Cancel", or "Retry" on the palette card, or
  "Cancel", "Retry", or the confirmed "Remove" in Settings. Plain tests in
  `src/app/tests/semantic_search.rs` cover some of these, without artifacts.
- Search ranking, indexing, and the real download need the real model. Never
  run `semantic-search-real-install` unless the user asks.
- Result count does not prove the thumbnails are visible. Only
  `search-composer` waits for preview decoding.
- Enter enables from the first-launch dialog. Enter does nothing on the install
  card or the Remove dialog.
- ⌘F does nothing while any modal is open, including the first-launch dialog.
