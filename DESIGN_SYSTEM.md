# Design System

## Purpose

Reference is a dense local-first desktop reference board. The design system protects a quiet, shortcut-friendly workspace where images and annotations stay primary and app chrome stays predictable, compact, and low-friction.

## Project Paths

- Tokens/styles: `src/theme.rs`
- Layout helpers: `src/ui_layout.rs`
- Shared UI patterns: `src/sidebar.rs`, `src/palette.rs`, `src/canvas/render.rs`
- Permanent preview: debug probe in `src/design_system_preview.rs`
- Verification entry point: `cargo run -- --verify-design-system --screenshot /tmp/reference-design-system.png`
- Temporary comparisons: add narrow debug-only probes in `src/app.rs` and remove them after the decision unless they become reusable verification states.

## Visual Vocabulary

Reference uses a dark, layout-first interface. Surfaces are quiet and compact; spacing and alignment do more work than borders or decoration. The canvas background is the dominant workspace, while sidebars, palettes, context menus, and toolbars should feel temporary and subordinate to arranging references.

The app favors low-radius rectangular controls, restrained contrast, and concise labels. Use warmer surface tones only where they help separate overlays from the canvas. Use emphasis sparingly: selected items, active focus, and currently actionable state can stand out; generic importance should not create new color roles.

Default to left-aligned content and titles. Center alignment should be an explicit exception for spatial canvas geometry, icon-only controls, or brief status elements where symmetry is the point.

## Tokens And Semantics

- `BACKGROUND` in `src/theme.rs`: use for the canvas and app-level backdrop. Do not use for grouped panels that need to read as controls.
- `SURFACE`: use for sidebars, drawers, resting controls, and grouped UI. Do not use as a selected state.
- `PALETTE_SURFACE`: use for the command/search palette panel only. Do not generalize it into a second default panel color.
- `SURFACE_ELEVATED`: use for hover states, selected rows, selected surfaces, and separation inside an existing panel. Do not use for ordinary resting chrome.
- `BORDER` and `BORDER_STRONG`: use for quiet separators and stronger control affordance. Prefer spacing before adding new borders.
- `TEXT`, `TEXT_MUTED`, `TEXT_SUBTLE`: use for primary labels, secondary values, and low-emphasis metadata respectively. Do not use subtle text for required instructions.
- `ACCENT`: use for active selection, focus, and the few states that need emphasis without adding a new hue. Do not use for generic decoration.
- `DESTRUCTIVE`, `DESTRUCTIVE_STROKE`, and `DESTRUCTIVE_FILL`: use for destructive labels, confirmation borders, and destructive confirmation actions. The base is `#CA3C25`; keep repeated row affordances quiet unless hovered or focused.
- Spacing constants: use `SPACE_1`/`SPACE_2`/`SPACE_3`/`SPACE_4`/`SPACE_6`/`SPACE_8` for 4/8/12/16/24/32px spacing. Do not reintroduce a 20px token. Values of 1-3px are allowed only as local optical corrections; map other near-token spacing back to the semantic scale unless a component has a documented reason.
- Stack spacing: use `SPACE_1` between repeated rows in one navigation/list stack when each row owns its internal padding; use `SPACE_2` between denser inline controls; use `SPACE_3` between a row stack and its immediate follow-up action; use `SPACE_4` before explanatory helper text or a new content group; use `SPACE_6` or larger only between major sections. Hover/focus state must not change allocated row size or stack spacing.
- Radius constants: use `RADIUS_NONE` for structural fills and rules, `RADIUS_MICRO` for tiny marks and thumbnails, `RADIUS_INLINE` for quiet inline settings controls, `RADIUS_COMPACT` for rows/chips/hints/inset groups, `RADIUS_CONTROL` for explicit buttons and canvas toolbars, and `RADIUS_PANEL` for floating panels and major overlays.
- Alignment helpers in `src/ui_layout.rs`: use `row_center_y` for mixed inline rows, `row_end_center_y_layout` for trailing action groups, `column_left` for default stacked content, `column_center_x` only for brief centered status/spatial previews, `centered_single` only for one child that should occupy the available rect, and `centered_pos` for manually painted content centered in a rect. Avoid ad hoc center math unless a component has a documented optical correction.

## Components And Patterns

- Sidebar: app-level settings and board navigation live in `src/sidebar.rs`. It should stay compact, left-anchored, and dismissible; do not turn it into a persistent inspector unless the product flow changes.
- Sidebar board row: use for board switching, rename, delete affordance, active-board indication, and shortcut hints. Treat it as a compact navigation row: active rows get a left rail and quiet fill, idle rows stay transparent, shortcuts read as metadata, and destructive delete language appears only when the full row rect is hovered or focused. Normal rows paint static text; mount text-editing controls only during rename so hover/focus cannot change row geometry. Do not reuse it for generic lists unless the list has board-level navigation semantics.
- Sidebar subcard: use for grouped settings inside the sidebar. Do not nest cards inside it.
- Quiet inline controls: use for compact setting choices and binary setting values in read-heavy panels. Only the active state gets a quiet frame; inactive choices should read as available text, not separate pills. Do not use this for canvas toolbars where the target needs to stay visually explicit.
- Destructive actions: use explicit destructive language and reserved destructive color in context menus and confirmations. Do not make repeated row-level delete affordances loud by default.
- Search palette: semantic search and library recall live in `src/search.rs` with reusable paint helpers in `src/palette.rs`. It should remain an overlay workflow, not a second sidebar.
- Canvas overlays and toolbars: annotation and edit controls live close to the selected item in `src/canvas/render.rs`. They should stay compact and avoid obscuring the reference material.
- Debug verification probes: keep screenshotable UI states in `src/app.rs` when they expose a broadly useful state. Keep one-off inspection probes narrow and remove them after use.

## Preview And Verification

- Preview location: `src/design_system_preview.rs`, exposed through `--verify-design-system` in debug builds.
- Command: `cargo run -- --verify-design-system --screenshot /tmp/reference-design-system.png`
- Sidebar board-row probes: `cargo run -- --verify-sidebar-boards --screenshot /tmp/reference-sidebar-boards.png` and `cargo run -- --verify-sidebar-boards-hover --screenshot /tmp/reference-sidebar-boards-hover.png`
- Destructive confirmation probes: `cargo run -- --verify-board-delete-confirmation --screenshot /tmp/reference-board-delete-confirmation.png` and `cargo run -- --verify-library-remove-confirmation --screenshot /tmp/reference-library-remove-confirmation.png`
- Required checks before publishing design-system changes: `cargo fmt`, `cargo check`, then capture and inspect the preview screenshot.
- When sharing screenshots in chat, use a unique filename or copy the verified screenshot to a timestamped `/tmp` path before embedding it. Reusing the same `/tmp/*.png` path can show stale cached images in the chat even when the file on disk is current.
- Supported mode: dark theme only.
- Screenshot artifacts are temporary evidence and should not be committed unless explicitly requested.

## Updating This Design System

Before changing shared UI, classify the change as local-only, reuse, extension, new primitive, token change, pattern change, exception, or conflict.

Update implementation, the debug preview, and this document together when shared visual semantics change. Local one-offs can stay local, but document intentional exceptions when future work is likely to mistake them for drift.

## Known Drift

- Some reusable paint helpers are private to their modules, which keeps ownership simple but limits direct preview reuse. Prefer extracting a helper only when a real app usage needs it too.

## Open Questions

- Should the design-system preview eventually show real sidebar and palette compositions by calling shared paint helpers, or stay focused on token and primitive semantics?
- Should annotation colors become documented semantic tokens, or remain canvas-domain values owned by annotation behavior?
- Temporary comparison: compact control consolidation is available with `cargo run -- --compare-compact-controls --screenshot /tmp/reference-compact-controls.png`. Option C is selected for sidebar settings; palette pills and canvas toolbar controls still need focused follow-up before migration.
- Temporary comparison: radius semantics are available with `cargo run -- --compare-radii --screenshot /tmp/reference-radii.png`. The selected merge collapses previous row/chip/group values into `RADIUS_COMPACT`.
- Temporary comparison: spacing scale consolidation is available with `cargo run -- --compare-spacing --screenshot /tmp/reference-spacing.png`. Option B is selected: keep `4/8/12/16/24/32`, retire `20` as a named token, and allow documented 1-3px optical corrections.
- Temporary comparison: sidebar board-row component variants are available with `cargo run -- --compare-board-rows --screenshot /tmp/reference-board-rows.png`; option B is selected. Use `--compare-board-rows-bottom` for a deterministic scrolled lower capture.
