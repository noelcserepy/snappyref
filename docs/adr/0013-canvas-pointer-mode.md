# Canvas pointer mode

SnappyRef exposes a **Pointer Mode** workspace setting with two values, **Mouse Mode** and **Trackpad Mode**, instead of trying to infer the user's input device or branching on platform. Mouse Mode is the default on every platform.

The target user is an artist on a Windows PC using a mouse. PureRef-style wheel-zoom plus `Alt`+LMB pan is what that user expects; egui's default smart inputs (which pan on wheel) feel wrong to them. Mac trackpad users get a one-click switch to native pinch/two-finger semantics. Automatic device detection is unreliable in practice (mixed setups, hot-plugged peripherals, generic HID quirks) and would flap, so we make the toggle explicit.

## Decision

- Add `pointer_mode: PointerMode { Mouse, Trackpad }` to **Workspace Settings**, default `Mouse` on every platform and for existing sessions deserialized without the field.
- **Mouse Mode** intercepts only true mouse-wheel notches (`MouseWheelUnit::Line | Page`) and routes them to zoom around the pointer (`1.1` factor per line, smooth via `1.1^(-Δy)`). Horizontal wheel and `Shift`+wheel are no-ops.
- **Mouse Mode** is permissive about trackpad inputs that happen to arrive: pinch (`Event::Zoom`) and `Point`-unit scroll still zoom and pan respectively, so users with hybrid setups never lose those affordances.
- **Trackpad Mode** is exactly current behavior: egui's `zoom_delta()` and `translation_delta()` smart inputs.
- `Alt`/`Option`+LMB always pans the canvas in both modes, even when the press lands on a placement. Mirrors PureRef.
- `MMB`+drag also pans in both modes.

## Consequences

- Existing trackpad-only users see no behavior change: their pinch and two-finger pan are preserved in both modes, and they never produce wheel-notch events.
- Existing Windows mouse users get the intended new behavior on first launch without migration UI.
- Switching pointer modes is one click and reversible; the cost of a wrong default is low.
- Modifier conflict: `Alt` was previously read only during rotation-handle drag (to bypass rotation snap). Pan starts on press-with-Alt, snap-bypass applies during an already-active rotation drag — the two gestures cannot overlap, so the reuse is safe.
- No auto-detection logic to maintain; future contributors should resist adding it.
