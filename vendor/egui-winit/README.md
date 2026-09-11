# Vendored egui-winit

This is a copy of [egui-winit 0.34.1](https://crates.io/crates/egui-winit/0.34.1)
from the [egui](https://github.com/emilk/egui) repository, licensed under
MIT or Apache-2.0 (see LICENSE-MIT and LICENSE-APACHE). `Cargo.toml` at the
repo root points `egui-winit` at this directory through `[patch.crates-io]`.

## Local patch

One change, in `src/lib.rs`, inside the paste branch of `on_keyboard_input`.
Upstream only pushes `egui::Event::Paste` when the clipboard holds non-empty
text. The patched version always pushes `Paste`, with an empty string when
there is no text. SnappyRef uses that event as the paste trigger and then
reads the clipboard itself, so an image-only clipboard (no text) still pastes.

## Refreshing on an egui bump

Download the matching `egui-winit` crate from crates.io, replace `src/` and
`Cargo.toml` here, reapply the paste change above, then diff the new `src/`
against the upstream crate to confirm it is the only difference.
