# Idle-safe animated image playback

SnappyRef supports animated image assets, but animation must not make a quiet board behave like a constantly active workload.

Perf profiling showed that a single visible animated GIF could keep the app repainting continuously after imports, embeddings, and backfill had settled. That made **Settled Idle** report true while CPU stayed high, which violates the idle target. The measured bottleneck was repaint pressure from passive animation, not the import pipeline itself.

## Decision

- Animated placements may be loaded and visible without actively advancing animation.
- An animated placement becomes an **Active Animated Image** only while it is selected or hovered.
- Visible-but-inactive animated placements draw their current frame and do not schedule animation repaint work.
- Active animation schedules repaint for the next frame delay instead of requesting immediate continuous repaint.
- Pathologically tiny animation delays are normalized so imported animations cannot request extremely high frame rates.
- Perf instrumentation must report visible animated image count separately from active animated image count.

## Consequences

- A board with passive animated assets can still reach **Settled Idle** without animation-driven repaint churn.
- Animated images no longer autoplay just because they are visible on the canvas.
- Hovering or selecting an animated placement is the explicit interaction that trades CPU for playback.
- Future animated-image work should preserve the distinction between visible animation support and active animation playback.
