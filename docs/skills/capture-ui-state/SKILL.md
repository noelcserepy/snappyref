---
name: capture-ui-state
description: Add temporary debug-only probes to eframe/egui apps so Codex can open overlays, enter edit modes, reset viewport state, and capture screenshots of arbitrary UI states without changing release behavior. Use when asked to inspect a screen, dialog, drawer, toolbar, search overlay, selection state, or any other in-app UI state by screenshot.
---

# Capture UI State

## Use This Skill

Use this skill when you need a screenshot of a specific app state and the state is easier to create from inside the app than through pixel-level interaction.

Prefer it for:

- Opening overlays, drawers, dialogs, or inspectors
- Showing editing modes, toolbars, or selection states
- Capturing canvas or viewport states after a reset
- Inspecting flows that need one or more internal state mutations before rendering

## Workflow

1. Reuse existing hooks first.
   - Search for existing screenshot flags, verification modes, debug commands, or test harnesses.
   - Prefer what is already present over adding a new path.

2. Make the target state deterministic.
   - Reset persisted or user-derived state when it affects the screenshot.
   - Force a known camera, viewport, or selection state if the target is spatially sensitive.
   - Seed only the minimal data needed to expose the UI you want.

3. Add a temporary debug-only probe.
   - Gate it behind `#[cfg(debug_assertions)]` or a feature flag that defaults off in release.
   - Prefer a startup probe or a direct state mutation in `new(...)`.
   - Use the app's own state methods when available instead of simulating pointer coordinates.
   - Name the probe narrowly, such as `--verify-search`, `--verify-textbox`, or `--verify-arrow`.

4. Let the UI settle before capture.
   - If the state depends on async work, wait for the relevant result or readiness signal.
   - If layout needs another frame, request a repaint and capture on the next frame.
   - If a toolbar or overlay only appears after focus changes, set focus explicitly before capture.

5. Capture with the app's existing screenshot path.
   - Reuse any built-in `--screenshot` or equivalent path.
   - Close the app after the screenshot is written if that matches existing behavior.

6. Inspect and iterate.
   - Open the screenshot and verify the intended UI is visible.
   - Adjust the probe if the target is off-screen, hidden, or clipped.
   - Remove the probe later unless it is broadly useful enough to keep.

## Probe Design Rules

- Keep release behavior unchanged.
- Prefer direct state setup over synthetic mouse paths.
- Reset the viewport before canvas or overlay captures unless the current camera is part of the state being inspected.
- Set editor focus explicitly when capturing text entry or editing toolbars.
- Wait one repaint after opening a floating panel, if needed, so layout and focus state stabilize.
- Use the smallest possible probe that produces the screenshot you need.

## Practical Pattern

When the app already has a screenshot hook, the smallest useful pattern is:

1. Add a debug-only enum variant or command flag.
2. In startup initialization, set the state you want to inspect.
3. If the view depends on camera position, reset the camera to a known pose.
4. Open the overlay/editor/drawer and request focus if needed.
5. Let the existing screenshot capture path do the rest.

## Output

Return the screenshot path and a short description of what is visible.
If the probe needed a special reset or delay, mention that briefly.
