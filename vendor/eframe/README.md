# Vendored eframe

This is a copy of [eframe 0.36.2](https://crates.io/crates/eframe/0.36.2)
from the [egui](https://github.com/emilk/egui) repository, licensed under
MIT or Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). `Cargo.toml` at the
repo root points `eframe` at this directory through `[patch.crates-io]`.

## Local patch

This carries [egui#8571](https://github.com/emilk/egui/pull/8571) on top of
0.36.2. It lets a `ViewportCommand::Screenshot` be
served from a window that is minimized or covered, which the AI control
`screenshot` tool needs. Three vendored crates carry it together: `eframe`,
`egui-wgpu`, and `egui-winit`. The upstream `RequestPaintWhileHidden` command
is left out because it lives in the `egui` crate, which is not vendored, and a
pending screenshot asks for the same paint.

The change here is in `src/native/wgpu_integration.rs` and
`src/native/glow_integration.rs`. A hidden window is painted when a pending
action wants the pixels. `src/web/web_painter_wgpu.rs` follows the changed
`CaptureState` signature in `egui-wgpu`.

## Refreshing the vendored crates

Refresh `eframe`, `egui-wgpu`, and `egui-winit` together to the matching
upstream version. Replace each crate's sources and manifest from its release,
then diff against that release to account for every local change. Update the
root egui-family dependency versions and `Cargo.lock` together with the vendor
versions, and update each README's version and provenance. Follow the
[repo check policy](../../AGENTS.md#build-check-test) for the shipping build.

Until the release contains [egui#8571](https://github.com/emilk/egui/pull/8571),
reapply its screenshot changes in all three crates. Keep the adaptation that
omits `RequestPaintWhileHidden` while `egui` itself is not vendored. Always
reapply the image-only clipboard paste change described in
[egui-winit](../egui-winit/README.md#local-patches), unless the new release
already implements that behavior.

Once the release contains #8571, remove the screenshot changes. `eframe` and
`egui-wgpu` can then return to crates.io by deleting their directories and
`[patch.crates-io]` entries. Keep `egui-winit` vendored while its paste fix is
still needed.

Build the debug app with `cargo build --locked --bin snappyref`, run
`docs/skills/inspect/scripts/mcp-drive.py docs/skills/inspect/scripts/mcp-all-tools.json`,
and read the report and screenshot replies to confirm covered-window capture.
This drive does not minimize the window. Verify minimized-window capture
separately when that path changes, and report it as unverified otherwise.
Also check image-only clipboard paste on the target platforms when refreshing
`egui-winit`.
