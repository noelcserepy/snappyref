# Vendored egui-winit

This is a copy of [egui-winit 0.36.2](https://crates.io/crates/egui-winit/0.36.2)
from the [egui](https://github.com/emilk/egui) repository, licensed under
MIT or Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). `Cargo.toml` at the
repo root points `egui-winit` at this directory through `[patch.crates-io]`.

## Local patches

The first is in `src/lib.rs`, inside the paste branch of `on_keyboard_input`.
Upstream only pushes `egui::Event::Paste` when the clipboard holds non-empty
text. The patched version always pushes `Paste`, with an empty string when
there is no text. SnappyRef uses that event as the paste trigger and then
reads the clipboard itself, so an image-only clipboard (no text) still pastes.

The second adds `ActionRequested::wants_paint` in `src/lib.rs`. It is this
crate's share of [egui#8571](https://github.com/emilk/egui/pull/8571), which
[vendor/eframe](../eframe/README.md) describes. It goes away with an egui
release that contains that change. The paste change stays.

## Refreshing on an egui bump

Download the matching `egui-winit` crate from crates.io, replace `src/` and
`Cargo.toml` here, reapply the paste change above, then diff the new `src/`
against the upstream crate to confirm what differs.
