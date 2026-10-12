# Vendored eframe

This is a copy of [eframe 0.36.2](https://crates.io/crates/eframe/0.36.2)
from the [egui](https://github.com/emilk/egui) repository, licensed under
MIT or Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). `Cargo.toml` at the
repo root points `eframe` at this directory through `[patch.crates-io]`.

## Local patch

This carries [egui#8571](https://github.com/emilk/egui/pull/8571), merged
after 0.36.2 and not yet released. It lets a `ViewportCommand::Screenshot` be
served from a window that is minimized or covered, which the AI control
`screenshot` tool needs. Three vendored crates carry it together: `eframe`,
`egui-wgpu`, and `egui-winit`. The upstream `RequestPaintWhileHidden` command
is left out because it lives in the `egui` crate, which is not vendored, and a
pending screenshot asks for the same paint.

The change here is in `src/native/wgpu_integration.rs` and
`src/native/glow_integration.rs`. A hidden window is painted when a pending
action wants the pixels. `src/web/web_painter_wgpu.rs` follows the changed
`CaptureState` signature in `egui-wgpu`.

## Dropping this copy

Once SnappyRef moves to an egui release that contains egui#8571, delete this
directory and its `[patch.crates-io]` line. Then run
`docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`
and confirm its `screenshot` calls still answer with the window covered.
