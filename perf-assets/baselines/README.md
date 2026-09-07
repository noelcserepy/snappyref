# Performance references

`macos-m4-pro-2026-09-07.json` contains 51 valid trials across 17 scenarios,
three trials per scenario. Measured on an Apple M4 Pro with 24 GiB RAM,
macOS 26.6.2, AC power, and the shipping optimized release build.

The source commit and binary/model/corpus/compiler fingerprints are in the
JSON. Each metric includes its median, minimum, and maximum. The local report at
`perf-assets/runs/refactor-baseline-published-20260907/evaluation.json` points
to the raw trial directories.

One static-import acquisition timed out after images and embeddings finished,
before an idle marker arrived. Three unchanged-binary repeats passed. The
cause remains unproven. The JSON records the rejected run, the complete static
replay, and the remaining board/library repetitions. It preserves the failed
report and lists all 51 selected trials. Future timeouts must remain failures,
not performance passes.

This is the full reference. Keep it unchanged. Regular profiling, including
refactors, defaults to fast with five cases repeated three times. Fast requires
a separately acquired matching reference; the full JSON cannot supply one.

Acquire and compare fast runs from the repo root:

```sh
python3 scripts/check-refactor-behavior.py
python3 scripts/perf_eval.py run --output perf-assets/runs/fast-before
# Make the change, repeat behavior checks, then:
python3 scripts/perf_eval.py run --output perf-assets/runs/fast-after
python3 scripts/perf_eval.py compare \
  perf-assets/runs/fast-before/evaluation.json \
  perf-assets/runs/fast-after/evaluation.json
```

Fast checks `import-1`, `import-20`, `board-0`, `board-1000`, and `library-1000`.
It stops after a 500 ms guard following settled idle and compares latency only.
It omits CPU/RSS, trickle, format/animation controls, intermediate and combined
sizes, and backfill. A fast pass is not full coverage. The prepared-command goal
is below 60 seconds, including setup and cleanup but excluding build and model
installation. Complete runtime remains unmeasured.

Use `--suite full` only when the user explicitly requests full, with a matching
full reference. Never run full automatically for refactors, releases, omitted
workloads, or inconclusive fast results.

Rerun a before-change baseline when the machine, toolchain, corpus, model, or
measurement settings differ. Keep builds and behavior tests out of timed runs.
See [perf-profiling](../../docs/skills/perf-profiling/SKILL.md) for the measurement
contract, scenarios, and regression budgets.

The animated case keeps its imported image selected and animating. Its CPU
is not a resting-board target. CPU readings of zero mean below the roughly
0.1 percentage point resolution. Filesystem cache is uncontrolled, usually warm.
RSS varies with native allocators and the OS. An earlier unchanged-binary replay of six
import trials stayed within 13 of 14 metric budgets. That calibration preceded
the auto-layout feature; the final 51-trial baseline includes it. That replay's
remaining RSS result overlapped its baseline's range, so the comparison reports inconclusive and
exits 2. It does not label that a regression or allow an automatic pass.
Preserve inconclusive results and investigate or acquire another complete
repeated run of the same profile. The 200 ms process-tree sampler can miss brief peaks.

This baseline covers startup, scripted import, embedding, and post-work CPU/RSS.
Inspect tests cover editing, search interactions, undo/redo, restart, and write
failure recovery. Drag/zoom latency, text-search latency, native external input,
Windows behavior, and networked installation/update flows need targeted checks
when a refactor touches them.

[Fast functional evidence](fast-validation-2026-09-07.json) records a loaded-desktop
15-trial run at 43.58 seconds and detection of a temporary 750 ms startup delay.
This is not the required ten-run idle-session calibration or a reusable fast reference.
