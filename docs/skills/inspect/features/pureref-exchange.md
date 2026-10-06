# PureRef exchange

Users open PureRef `.pur` boards in SnappyRef and write SnappyRef boards back
out as `.pur` files. A dropped `.pur` file becomes a new active board that
keeps each image's position, scale, rotation, crop, and stacking. Export writes
the active board's images and textboxes to a file the user picks.

## Sub-features

- `pureref-import` turns a dropped `.pur` file into a new board named after the
  file. Repeated images share one asset, and each distinct crop is stored as
  its own cropped original. Notes become textboxes above the images. The
  status line counts images and notes, flipped images, and missing images.
- `pureref-import-undo` uses ⌘Z / Ctrl+Z to remove the imported board and
  return to the previous one, like any board create.
- `pureref-export` uses ⇧⌘E / Ctrl+Shift+E. It opens the native save panel and
  writes every loaded image and textbox on the active board. Lines, arrows,
  and images PureRef can't open (SVG, AVIF, RAW and others) are left out and
  counted in the status line.

## How to get to it (user POV)

- Drag a `.pur` file from Finder or Explorer onto the window. Hovering it shows
  no placeholder.
- Press ⇧⌘E / Ctrl+Shift+E and choose where to save. "Export board to PureRef"
  is listed on the Settings "Keys" page.

## Driving it with inspect

Preconditions:

- `doctor.sh` exits 0.

- **Import.** Run `run-scenario.sh pureref-import`. It writes `Moodboard.pur`
  into the artifact dir with the codec, then drives a file hover and a drop.
  The file holds a repeated image, a rotated and scaled item, a cropped item,
  a mirrored item, and a two-line note. Assertions check corner positions
  against PureRef's transforms, shared and cropped assets, stack order, and
  the textbox. "the board is durable" reads the board back after a flush. ⌘Z
  is driven. Frames are `dropped`, `loaded`, and `undone`.
- **Export and round trip.** Run `run-scenario.sh pureref-export`. The board is
  seeded in code with two rotated images, a textbox, and a line. ⇧⌘E is
  driven, but the save panel is replaced by a fixed path, `exported.pur` in the
  artifact dir. Assertions read the file back with the codec, check embedded
  bytes and transforms, then drop the file back in and compare geometry.
  Frames are `board` and `reimported`.

## Gotchas

- The native save panel has no scenario. Scenarios replace
  `SnappyRefApp::pureref_save_path`. Linux builds have no save panel, so
  export does nothing there.
- Neither scenario opens a file in PureRef itself. Compatibility rests on the
  codec's own tests.
- Mirrored images come in unmirrored, and note rotation is dropped. Textbox
  size is estimated from the note's text, so a round trip keeps position and
  text size but not the box size.
- Image opacity and PureRef groups are ignored on import.
- Import and export run on a background thread. Step the harness until the
  status starts with "Imported" or "Exported" before asserting.
- Drops of `.pur` files work without an import worker, but the imported images
  only load with one.
