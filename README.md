# SnappyRef

A local reference-board app for macOS and Windows. Drop images on a canvas,
arrange them, and find them again across every board with on-device search.

<!-- Screenshot placeholder. Inspect scenarios seed solid-color rectangles, not
real boards. Add a real capture at assets/readme/board.png and link it here. -->

## What it does

- Boards. Each named Board keeps its own placements, annotations, groups,
  viewport, background, and undo history.
- Import. Drag files or browser image URLs onto the canvas, or paste. Pasting
  PureRef clipboard content imports the images it references.
- Formats. JPEG, PNG, WebP, GIF, TIFF, BMP, SVG, OpenEXR, and JPEG XL on both
  platforms. macOS also decodes AVIF, HEIC, PSD, and JPEG 2000 through the
  system decoders.
- Editing. Move, resize, uniform scale, rotate, crop, duplicate, group, change
  stack order, and draw arrows, lines, and textboxes.
- Layout. Snapping guides, compact, row, and column arrange, group auto-layout,
  and a configurable gap between placements.
- Library. Every imported image lands in one local Library shared by all
  boards. Exact duplicates reuse the existing Asset.
- Semantic Search. Optional. Cmd+F (Ctrl+F) searches the whole Library by
  text or by an image, and results drag straight onto the board.
- Shortcuts. Every action has a key. The sidebar (Cmd+B / Ctrl+B) lists them
  and holds settings, boards, and the app version.
- Updates. The app checks a signed release manifest and installs newer
  versions itself. Your Library and model files stay put.

## Download

Releases are at <https://github.com/noelcserepy/snappyref-dist/releases>.
Each release has `snappyref-mac-<version>.zip` and
`snappyref-windows-<version>.zip`. Unzip, keep the `SnappyRef` folder
together, and run the app from inside it. The Library is stored beside the app
when that folder is writable, otherwise in your user app-data directory.

First launch:

- macOS 14 or newer. The app is ad-hoc signed, not notarized, so a plain
  double-click is blocked. Try opening `SnappyRef.app`, then go to System
  Settings > Privacy & Security and click Open Anyway. Confirm Open when
  prompted. See [Apple's instructions](https://support.apple.com/en-gb/102445).
- Windows. The exe is unsigned. SmartScreen shows "Windows protected your PC".
  Click More info, then Run anyway.

After that, updates arrive in-app. The app verifies `latest.json` against an
embedded ed25519 public key before it downloads or installs anything.

## Semantic Search

Off until you enable it. The app asks once on first launch, and Settings in the
sidebar can turn it on later. Enabling downloads one model package, about
254 MB, from the releases page. The model is SigLIP2
(`onnx-community/siglip2-base-patch16-224-ONNX`, int8) and runs on-device
through ONNX Runtime. Queries and embeddings never leave your machine. The
model lives in a `models/` folder next to the Library, and app updates do not
touch either.

## Build from source

You need Rust stable and a C compiler. SQLite is compiled from source by
`rusqlite` and ONNX Runtime is fetched as a prebuilt binary by `ort` during the
build. The default `turbojpeg-native` feature links libjpeg-turbo for fast
JPEG decoding, and that is the one library you install yourself.

macOS. `.cargo/config.toml` sets `TURBOJPEG_SOURCE=pkg-config`, so the
Homebrew package is enough:

```sh
brew install jpeg-turbo pkg-config
cargo build --locked --release
```

Windows. Either install libjpeg-turbo through vcpkg and point the build at it,
the same way the release workflow does:

```powershell
vcpkg install libjpeg-turbo:x64-windows-static-md
$env:TURBOJPEG_SOURCE = "explicit"
$env:TURBOJPEG_STATIC = "1"
$env:TURBOJPEG_LIB_DIR = "<vcpkg root>\installed\x64-windows-static-md\lib"
$env:TURBOJPEG_INCLUDE_DIR = "<vcpkg root>\installed\x64-windows-static-md\include"
cargo build --locked --release
```

or skip it with `cargo build --locked --release --no-default-features`. JPEGs
then decode through the pure-Rust `image` crate, which is slower on large
files.

Linux is not a supported target today. Nobody builds or tests there.

To run a development build without touching a real Library, give it its own
data directory:

```sh
SNAPPYREF_DATA_ROOT="$(mktemp -d)" cargo run --locked --bin snappyref
```

## Development checks

Point git at the repo hooks once per clone:

```sh
git config core.hooksPath .githooks
```

The [pre-push hook](.githooks/pre-push) runs `cargo fmt --check`, clippy
with warnings denied, `cargo check --locked`, and
`cargo test --locked update_preservation_smoke`. `cargo test --locked`
runs the full suite. CI runs clippy, `cargo check` and the full suite on macOS
and Windows for every pull request, plus `cargo fmt --check` on macOS. UI
behavior is verified through inspect scenarios; list them with
`docs/skills/inspect/scripts/run-scenario.sh --list`.

## Code map

[AGENTS.md](AGENTS.md) has the process model, the module map, and the repo
rules. Product contracts live under [.specs/](.specs/).

## License

MIT. See [LICENSE](LICENSE).

Third-party licenses are in `THIRD_PARTY_NOTICES.txt`, next to `snappyref.exe`
on Windows and in `SnappyRef.app/Contents/Resources` on macOS.
