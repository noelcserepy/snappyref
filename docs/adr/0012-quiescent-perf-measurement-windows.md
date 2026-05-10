# Quiescent perf measurement windows

SnappyRef perf results distinguish app work state from measured CPU and memory outcomes.

Earlier idle CPU summaries averaged a short window after **Settled Idle**. That mixed import/embedding CPU accounting decay, late OS/window events, and shutdown work with true steady idle. The result made a quiet app look like it still had a persistent CPU load.

## Decision

- Keep **Time to Idle** as the headline work-drain metric from scenario start to **Settled Idle**.
- Measure **Idle Tail** separately for the first post-idle seconds.
- Measure **Quiescent Idle** after the tail and before shutdown.
- Measure CPU using process CPU-time deltas for the quiescent window, with `ps %cpu` retained only as a secondary sample.
- Measure **Quiescent Memory** over the same quiescent window.
- Exclude app shutdown from idle and memory target windows.
- Use board-size idle runs with 0, 1, 20, 100, and 1000 placements to distinguish per-placement retained memory from per-asset retained memory.

## Consequences

- The idle CPU target applies to **Quiescent Idle**, not **Idle Tail**.
- **Idle Tail** remains useful diagnostic data but should not drive steady-state CPU decisions.
- **Quiescent Memory** should be optimized before **Time to Idle**, because startup/import speedups can otherwise hide retained-memory growth.
- Perf reports must keep CPU and memory windows aligned so trade-offs are visible.
