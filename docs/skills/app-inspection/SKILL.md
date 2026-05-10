---
name: app-inspection
description: |
  Inspect, verify, or capture SnappyRef UI state using existing debug probes and deterministic screenshots. Use when asked to inspect the app, verify a UI state, capture a screenshot, add a temporary probe, or reproduce a visual state.
user-invocable: true
argument-hint: "[ui-state-or-probe]"
---

# App Inspection

Use this skill when inspecting the app, verifying a UI state, or adding a temporary probe to capture a screenshot.

## Inspecting The App

- Prefer the existing debug probes before adding anything new.
- Current probes live in `src/app.rs`:
  - `--verify-search=<query>`
  - `--verify-arrow`
  - `--verify-textbox`
- Use the built-in screenshot path with a file target, for example:
  - `cargo run -- --verify-search=boat --screenshot /tmp/snappyref-search.png`
  - `cargo run -- --verify-arrow --screenshot /tmp/snappyref-arrow.png`
  - `cargo run -- --verify-textbox --screenshot /tmp/snappyref-textbox.png`
- Inspect the resulting PNG directly before changing anything else.
- If a state depends on persisted viewport or session data, reset it inside the probe so the screenshot is deterministic.

## Creating New Probes

- Add probes only when the existing ones cannot expose the state you need.
- Keep probes debug-only with `#[cfg(debug_assertions)]` or an off-by-default feature flag.
- Prefer a startup probe in `src/app.rs` over synthetic mouse input.
- Set the target state directly through app methods or state fields.
- If the UI is spatially sensitive, reset camera, selection, or session state before capture.
- If the target is a floating overlay, text editor, or toolbar, request focus explicitly and wait one repaint if needed.
- Reuse the app's existing screenshot flow instead of inventing a second capture system.
- Remove the probe when you are done unless it is broadly useful.

## Implementation Pattern

- Extend the existing verification enum or command parsing in `src/app.rs`.
- Seed the app state in `SnappyRefApp::new(...)`.
- Let the current `ScreenshotCapture` path write the PNG and close the app.
- Run `cargo check` after changes.
- Capture a screenshot and confirm the intended state is visible.

## Guardrails

- Do not change release behavior for inspection work.
- Do not rely on pixel coordinates if a direct state mutation exists.
- Do not leave probe logic spread across unrelated files.
- Keep probe names narrow and descriptive so they are easy to reuse later.

## Notes

- The semantic-search model is downloaded on first use; treat the first run as potentially slow.
