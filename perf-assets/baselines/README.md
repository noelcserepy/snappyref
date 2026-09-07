# Refactor baseline

`macos-m4-pro-2026-09-07.json` contains 51 valid trials across 17 scenarios,
three trials per scenario. Measured on an Apple M4 Pro with 24 GiB RAM,
macOS 26.6.2, AC power, and the shipping optimized release build.

The source commit and binary/model/corpus/compiler fingerprints are in the
JSON. Each metric includes its median, minimum, and maximum. Raw artifacts
remain local under `perf-assets/runs/refactor-baseline-20260907-verified`.

Run the next comparison from the repo root:

```sh
python3 scripts/check-refactor-behavior.py
python3 scripts/perf_eval.py run --suite full --output perf-assets/runs/after
python3 scripts/perf_eval.py compare \
  perf-assets/baselines/macos-m4-pro-2026-09-07.json \
  perf-assets/runs/after/evaluation.json
```

Rerun a before-change baseline when the machine, toolchain, corpus, model, or
measurement settings differ. Keep builds and behavior tests out of timed runs.
See [perf-profiling](../../docs/skills/perf-profiling/SKILL.md) for the measurement
contract, scenarios, and regression budgets.

The animated case keeps its imported image selected and animating. Its CPU
is not a resting-board target. CPU readings of zero mean below the roughly
0.1 percentage point resolution. Filesystem cache is uncontrolled, usually warm.
RSS varies with native allocators and the OS. An unchanged-binary replay of six
import trials stayed within 13 of 14 metric budgets. The remaining RSS result
overlapped the baseline's range, so the comparison reports inconclusive and
exits 2. It does not label that a regression or allow an automatic pass.
Repeat or investigate inconclusive results before merging a refactor. The 200 ms process-tree sampler can miss brief peaks.

This baseline covers startup, scripted import, embedding, and post-work CPU/RSS.
Inspect tests cover editing, search interactions, undo/redo, restart, and write
failure recovery. Drag/zoom latency, text-search latency, native external input,
Windows behavior, and networked installation/update flows need targeted checks
when a refactor touches them.
