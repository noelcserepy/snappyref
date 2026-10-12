# Vendored egui-wgpu

This is a copy of [egui-wgpu 0.36.2](https://crates.io/crates/egui-wgpu/0.36.2)
from the [egui](https://github.com/emilk/egui) repository, licensed under
MIT or Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). `Cargo.toml` at the
repo root points `egui-wgpu` at this directory through `[patch.crates-io]`.

## Local patch

This carries [egui#8571](https://github.com/emilk/egui/pull/8571), merged
after 0.36.2 and not yet released. It lets a `ViewportCommand::Screenshot` be
served from a window that is minimized or covered, which the AI control
`screenshot` tool needs. Three vendored crates carry it together: `eframe`,
`egui-wgpu`, and `egui-winit`. The upstream `RequestPaintWhileHidden` command
is left out because it lives in the `egui` crate, which is not vendored, and a
pending screenshot asks for the same paint.

The change here is in `src/capture.rs` and `src/winit.rs`. A capture renders
to its own texture, so it no longer needs a surface texture, which a covered
or minimized window does not get.

## Dropping this copy

Once SnappyRef moves to an egui release that contains egui#8571, delete this
directory and its `[patch.crates-io]` line. Then run
`docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`
and confirm its `screenshot` calls still answer with the window covered.
