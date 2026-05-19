# Design System

## Project Paths

- Tokens/styles: `src/theme.rs`
- Layout helpers: `src/ui_layout.rs`
- Shared UI patterns: `src/sidebar.rs`, `src/palette.rs`, `src/canvas/render.rs`
- Visual verification: use `docs/skills/inspect/SKILL.md` and inspect scenarios/artifacts.
- Temporary comparisons: add the narrowest inspect scenario observation or assertion, then remove one-off instrumentation after the decision unless it becomes a reusable inspect primitive.

## Design Decisions

### Visual Posture

SnappyRef is a dense local-first desktop reference board. The design system protects a quiet, shortcut-friendly workspace where images and annotations stay primary and app chrome stays predictable, compact, and low-friction.

SnappyRef uses a dark, layout-first interface. Surfaces are quiet and compact; spacing and alignment do more work than borders or decoration. The canvas background is the dominant workspace, while sidebars, palettes, context menus, and toolbars should feel temporary and subordinate to arranging references.

Use emphasis sparingly. Selected items, active focus, and currently actionable state can stand out; generic importance should not create new color roles.

Supported mode: dark theme only.

### Layout And Spacing

Default to left-aligned content and titles. Center alignment should be an explicit exception for spatial canvas geometry, icon-only controls, or brief status elements where symmetry is the point.

Use `SPACE_1`/`SPACE_2`/`SPACE_3`/`SPACE_4`/`SPACE_6`/`SPACE_8` for 4/8/12/16/24/32px spacing. Do not reintroduce a 20px token. Values of 1-3px are allowed only as local optical corrections; map other near-token spacing back to the semantic scale unless a component has a documented reason.

Use `SPACE_1` between repeated rows in one navigation/list stack when each row owns its internal padding. Use `SPACE_2` between denser inline controls. Use `SPACE_3` between a row stack and its immediate follow-up action. Use `SPACE_4` before explanatory helper text or a new content group. Use `SPACE_6` or larger only between major sections. Hover/focus state must not change allocated row size or stack spacing.

Use `row_center_y` for mixed inline rows, `row_end_center_y_layout` for trailing action groups, `column_left` for default stacked content, `column_center_x` only for brief centered status/spatial previews, `centered_single` only for one child that should occupy the available rect, and `centered_pos` for manually painted content centered in a rect. Avoid ad hoc center math unless a component has a documented optical correction.

### Surfaces And Chrome

The app favors low-radius rectangular controls, restrained contrast, and concise labels. Use warmer surface tones only where they help separate overlays from the canvas.

Use `RADIUS_NONE` for structural fills and rules, `RADIUS_MICRO` for tiny marks and thumbnails, `RADIUS_INLINE` for quiet inline settings controls, `RADIUS_COMPACT` for rows/chips/hints/inset groups, `RADIUS_CONTROL` for explicit buttons and canvas toolbars, and `RADIUS_PANEL` for floating panels and major overlays.

Prefer spacing before adding new borders.

Do not nest cards inside sidebar subcards.

### Typography

Use `TEXT` for primary labels, `TEXT_MUTED` for secondary values, and `TEXT_SUBTLE` for low-emphasis metadata. Do not use subtle text for required instructions.

Keep labels concise and chrome subordinate to the reference material.

### Color Semantics

`BACKGROUND` is for the canvas and app-level backdrop. Do not use it for grouped panels that need to read as controls.

`SURFACE` is for sidebars, drawers, resting controls, and grouped UI. Do not use it as a selected state.

`PALETTE_SURFACE` is for the command/search palette panel only. Do not generalize it into a second default panel color.

`SURFACE_ELEVATED` is for hover states, selected rows, selected surfaces, and separation inside an existing panel. Do not use it for ordinary resting chrome.

`BORDER` and `BORDER_STRONG` are for quiet separators and stronger control affordance.

`ACCENT` is for active selection, focus, and the few states that need emphasis without adding a new hue. Do not use it for generic decoration.

`ACCENT_BLUE` and `ACCENT_ORANGE` are secondary accent candidates for explicit canvas guide or comparison states. Keep them out of generic chrome unless the visual role is named and tested.

`GREEN` is the only green token. Use it for green guide states and indexed/ready semantic-search status in the palette footer. Do not create a second success or decorative green.

`DESTRUCTIVE`, `DESTRUCTIVE_STROKE`, and `DESTRUCTIVE_FILL` are for destructive labels, confirmation borders, and destructive confirmation actions. The base is `#CA3C25`; keep repeated row affordances quiet unless hovered or focused.

`QUIET_INSET_FILL`, `QUIET_INLINE_ACTIVE_FILL`, and `QUIET_INLINE_HOVER_FILL` are low-contrast fills for sidebar subcards, quiet inline choices, and compact settings inputs. Use `QUIET_INLINE_*` metrics for read-heavy settings controls so selected, hover, and field states keep one density.

### Components And Patterns

Sidebar app-level settings and board navigation live in `src/sidebar.rs`. The sidebar should stay compact, left-anchored, and dismissible. Do not turn it into a persistent inspector unless the product flow changes.

Sidebar board rows are for board switching, rename, delete affordance, active-board indication, and shortcut hints. Treat them as compact navigation rows: active rows get a left rail and quiet fill, idle rows stay transparent, shortcuts read as metadata, and destructive delete language appears only when the full row rect is hovered or focused. Normal rows paint static text; mount text-editing controls only during rename so hover/focus cannot change row geometry. Do not reuse sidebar board rows for generic lists unless the list has board-level navigation semantics.

Sidebar subcards are for grouped settings inside the sidebar. Do not nest cards inside them.

Quiet inline controls are for compact setting choices and binary setting values in read-heavy panels. Only the active state gets a quiet frame; inactive choices should read as available text, not separate pills. Do not use quiet inline controls for canvas toolbars where the target needs to stay visually explicit.

Destructive actions use explicit destructive language and reserved destructive color in context menus and confirmations. Do not make repeated row-level delete affordances loud by default.

Semantic search and library recall live in `src/search.rs` with reusable paint helpers in `src/palette.rs`. The search palette should remain an overlay workflow, not a second sidebar.

Annotation and edit controls live close to the selected item in `src/canvas/render.rs`. Canvas overlays and toolbars should stay compact and avoid obscuring the reference material.

Image selection borders use the `TEXT` color, 2px stroke, square corners, and zero padding from the image bounds. Use this same border language for selected search result thumbnails in `src/palette.rs`, selected canvas Placements, and search drag previews in `src/canvas/render.rs`. Search results are library-wide Asset Results and do not highlight existing canvas Placements. Do not use rounded corners, expanded outlines, alternate selected colors, or thicker lead/hover variants for image selection.

Use inspect scenarios for screenshot-backed visual checks. Do not add startup debug flags or probe-only app states for design review.

### Interaction, State, And Accessibility

Hover/focus states must not change row geometry, allocated row size, or stack spacing.

Selected rows and surfaces use quiet fill or elevated surface treatment before louder accents. Active focus may use `ACCENT` when the interaction needs clear emphasis.

Repeated row-level destructive affordances stay quiet unless hovered or focused. Confirmation actions use explicit destructive labels and destructive color.

### Motion

No reusable motion language is currently documented. Do not add animation as generic decoration.

### Responsive And Density

SnappyRef is a dense desktop app. Preserve compact controls, predictable chrome, and shortcut-first flows.

Constrained surfaces such as sidebars, palettes, dialogs, and canvas toolbars should fit without text overflow, clipped focus rings, hover layout shift, or unexpected horizontal scrolling.
