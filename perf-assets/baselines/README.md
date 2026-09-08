# Performance references

[The fast reference](macos-m4-pro-fast-2026-09-07.json) contains all 15 trials,
three per selected case, acquired from clean source `eebf2f2`. It retains the
matching contract, fingerprints, trial metrics/states, aggregates and raw paths.
A separate same-binary control passed every comparison budget.

External complete-command times were 43.739 seconds for the reference and
44.159 seconds for the control, including interpreter startup, setup, seed,
fixtures, trials, cleanup and reporting. Both passed without failures or
60-second overruns. Models/corpus were already installed; no builds or tests
overlapped acquisition. Ordinary WindowServer, Zen and Codex activity remained.
The JSON records observed conditions and each command. This pair is one session,
not the ten-run, two-session runtime/noise calibration or a sub-minute guarantee.

Compare a prepared candidate with the committed fast reference:

```sh
python3 scripts/perf_eval.py run --binary target/release/snappyref \
  --output perf-assets/runs/fast-candidate
python3 scripts/perf_eval.py compare \
  perf-assets/baselines/macos-m4-pro-fast-2026-09-07.json \
  perf-assets/runs/fast-candidate/evaluation.json
```

Supply `--models` and `--assets` when prerequisites are outside the default
locations. Comparison requires the same machine, model, corpus, toolchain and
measurement contract. Build the shipping release binary before prepared timing.

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

See [perf-profiling](../../docs/skills/perf-profiling/SKILL.md) for acquisition,
measurement contracts, scenario coverage, and regression budgets. Fast compares
latency only. These references do not establish a prepared-command runtime
guarantee; the ten-run calibration remains outstanding.

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
That earlier functional check is not a reference. The separately acquired fast
reference above is comparable; ten-run calibration remains outstanding.
