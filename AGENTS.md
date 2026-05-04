# Repo Inspection Guide

Use this repo-local guide when inspecting the app, verifying a UI state, or adding a temporary probe to capture a screenshot.

## Repo Principles

- Optimize for speed first. Prefer changes that keep startup and core interactions fast.
- Favor layout over decoration. Use spacing, alignment, and positioning before borders or fills.
- Keep interactions shortcut-driven and low-friction.
- Use sensible defaults so repeated flows happen automatically.
- Keep the code and UI simple. If a solution feels complex, look for a smaller one first.

## Inspecting The App

- For Computer Use/manual UI testing, launch the app through the dev bundle wrapper so macOS exposes a bundle identifier:
  - `REFERENCE_LIBRARY_ROOT=/Users/noelcserepy/dev/reference/library scripts/run-dev-app.sh`
  - Target Computer Use at `dev.noelcserepy.snappyref.dev`.
  - Use `REFERENCE_LIBRARY_ROOT` when the test needs the existing repo-local image library; otherwise the wrapper starts with its own empty `target/SnappyRefDev.app` library.
- Prefer the existing debug probes before adding anything new.
- Current probes live in `src/app.rs`:
  - `--verify-search=<query>`
  - `--verify-arrow`
  - `--verify-textbox`
- Use the built-in screenshot path with a file target, for example:
  - `cargo run -- --verify-search=boat --screenshot /tmp/reference-search.png`
  - `cargo run -- --verify-arrow --screenshot /tmp/reference-arrow.png`
  - `cargo run -- --verify-textbox --screenshot /tmp/reference-textbox.png`
- Inspect the resulting PNG directly before changing anything else.
- If a state depends on persisted viewport or session data, reset it inside the probe so the screenshot is deterministic.

## Creating New Probes

- Add probes only when the existing ones cannot expose the state you need.
- Keep probes debug-only with `#[cfg(debug_assertions)]` or an off-by-default feature flag.
- Prefer a startup probe in `src/app.rs` over synthetic mouse input.
- Set the target state directly through app methods or state fields.
- If the UI is spatially sensitive, reset camera, selection, or session state before capture.
- If the target is a floating overlay, text editor, or toolbar, request focus explicitly and wait one repaint if needed.
- Reuse the app’s existing screenshot flow instead of inventing a second capture system.
- Remove the probe when you are done unless it is broadly useful.

## Implementation Pattern

- Extend the existing verification enum or command parsing in `src/app.rs`.
- Seed the app state in `ReferenceApp::new(...)`.
- Let the current `ScreenshotCapture` path write the PNG and close the app.
- Run `cargo check` after changes.
- Capture a screenshot and confirm the intended state is visible.

## Guardrails

- Do not change release behavior for inspection work.
- Do not rely on pixel coordinates if a direct state mutation exists.
- Do not leave probe logic spread across unrelated files.
- Keep probe names narrow and descriptive so they are easy to reuse later.

## Testing Notes

- The semantic-search model is downloaded on first use; treat the first run as potentially slow.
- Use `cargo check` after probe or UI changes before trusting a screenshot.

## Pre-push Checks

- CI runs `cargo fmt --check` and will fail on unformatted code. Always run `cargo fmt` before pushing.
- Run `cargo check` (and any tests you touched) locally before pushing so CI does not catch basic regressions.
