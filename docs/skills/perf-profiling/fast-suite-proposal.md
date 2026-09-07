# Fast performance suite proposal

Provisional planning only. Implementation is blocked on the user's answers below.
No new performance requirement is approved by this document. Keep the full suite
and committed reference baseline unchanged. This proposal belongs to its draft PR;
after decisions settle, move lasting usage/contract facts into the perf-profiling
skill and runner, then remove this temporary proposal.

## Outcome and evidence

Recommend a frequent release-app latency check with five cases and three fresh
processes per case. Aim below 60 seconds on the baseline Mac with prerequisites
ready. This is a hypothesis to validate, not a measured runtime promise.

Inspected main at `553e4ab704875ff499d64403750749cb2fc5eef6`, the merge of
[PR #200](https://github.com/noelcserepy/snappyref/pull/200).
Primary local evidence at that revision:

- `scripts/perf_eval.py`, especially `run_case`, `evaluate`, `compare`, and `main`.
- `scripts/_perf_sampling.py`, phase joins and process-tree sampling.
- `scripts/_perf_library_seed.py`, real-schema fixtures and APFS clone fallback.
- `src/app/perf_scenario.rs`, `finish_perf_scenario_if_idle` and paint markers.
- `scripts/check-refactor-behavior.py`, separate behavior/build/inspect checks.
- `docs/skills/perf-profiling/SKILL.md` and `perf-assets/baselines/README.md`.
- `perf-assets/baselines/macos-m4-pro-2026-09-07.json`, including rejected acquisition
  and calibration notes. Its measured app source is `ba4ee57`, not the merge SHA.

The committed baseline has aggregates and acquisition paths, not a `runs` array.
Read-only inspection also used the 15 selected raw `result.json` and `samples.csv`
files listed in its acquisition table under
`/Users/noelcserepy/dev/snappyref/perf-assets/runs/`, plus the original seed trial.
No build, behavior test, benchmark, or Windows access was performed for planning.
No external research was needed beyond repository PR metadata.

Observed full behavior: 17 cases x 3 repetitions. Each trial samples until roughly
17 seconds after settled idle. The 5..15 second window measures CPU and retained
RSS; tree peak covers launch through collection. The 51 holds alone cost 867
seconds. Scaling adds a seed process with the same hold. Full launch-to-idle
medians sum to 167.9 seconds over three repetitions, before holds and setup.

## Proposed scope and selection

All entries below are recommendations awaiting intent approval.

| Case | Idle median / observed max, seconds | Useful detection | Coverage sacrificed |
| --- | ---: | --- | --- |
| `import-1` | 1.016 / 1.040 | Small JPEG paint, fresh-process model load and demand completion | Other formats in isolation |
| `import-20` | 2.236 / 2.289 | Mixed batch paint and demand embedding drain | Static-only/animation controls, trickle reuse |
| `board-0` | 1.404 / 1.484 | Empty Board startup with one indexed Library asset | Truly empty first-install Library |
| `board-1000` | 3.543 / 3.550 | Placement cardinality with one shared indexed asset | Scaling curve and distinct-image residency |
| `library-1000` | 1.391 / 1.399 | Indexed Library cardinality with empty Board | Library/Board interaction and missing embeddings |

Measure launch-to-first-frame, launch-to-first-visible when applicable,
queue-to-all-visible for imports, and launch-to-settled-idle. Preserve marker
origins. Startup first-visible only proves at least one paintable placement;
startup does not require all 1,000 placements painted. Do not rename it all-visible.
Frame means submitted paint, not display presentation. Launch-to-idle includes
scheduling and the existing 500 ms idle qualification, not pure inference time.

This can expose large startup, decoding, batch completion, indexing, and isolated
cardinality regressions. It cannot prove smooth pan/zoom/drag, input latency,
search latency/relevance, network installation/update speed, cold-disk startup,
or Windows performance. Those are also outside the current full timing suite.
Synthetic indexed assets share pixels and seed vectors; they are cardinality
fixtures, not a representative 1,000-photo collection.

Keep these in full or an explicit existing case run: `trickle-import`, format and
animation controls, intermediate sizes, combined Library/Board sizes, and
`library-20-backfill`. Trickle previously caught a cached-completion hang, so its
exclusion is consequential. The current behavior suite stays separate; its
success does not establish release-app timing or replace that trickle workload.

## Proposed measurement and comparison contract

Extend the existing evaluator with one named fast profile. Reuse launch,
provenance, fixture, completion, and cleanup code. Avoid a second sampler or
runner. Keep the existing full profile, defaults, artifacts, and comparison
requirements intact.

Fast uses shipping release/default features, private libraries/process groups,
installed models, nonactivating windows, cleared overrides, and shipping helper
policies. Retain the same 200 ms sampling cadence initially to limit observer
changes. Raw early RSS may remain diagnostic, but omit CPU, retained RSS and
peak-tree RSS from fast comparison. A truncated peak is a different quantity.

Propose stopping 500 ms after the current settled-idle marker, with that summary
still present and no later work-resumed event. Resumed work restarts qualification.
This extra guard is not an idle CPU/RSS window or a guarantee against later work.
The app already qualifies idle for 500 ms before emitting the marker. Preserve
its 20-second hold setting and terminate externally after the fast guard; do not
change shipping model retention or helper grace to manufacture speed.

Validate required clocks, paint/loading counts, worker and embedding completion,
fixture cardinality and persisted state before accepting each trial. Preserve
startup's existing visibility semantics. Separate common completion checks from
full-only sampling-window checks; never merely disable validation for fast.
Require coherent summary/events and final database state. A missing marker,
resumed/incomplete work, timeout, nonzero app exit, failed embedding, or partial
report cannot become a pass. Cleanup must finish before the next fresh process.

Use the fast completion path for the untimed-in-metrics seed too. Otherwise its
existing 18.5-second launch-to-last-sample cost consumes much of the savings.
Seed work still counts in the proposed command runtime. Generate fixtures from
the tested binary's schema, never from an old branch's live library. No persistent
fixture cache in the initial design.

Record an explicit profile/measurement version, stop rule, metric set, case set,
repeats, fixture version and existing machine/model/corpus/build settings.
Compare fast only against a separately acquired matching fast reference. Reject
fast/full, mismatched windows, machines, fixtures, metrics, or missing cases.
Do not slice the published full baseline into a purported fast baseline. Its
numbers here are runtime estimates only. Shared latency names do not prove that
shorter observation and changed seed scheduling preserve distributions.

Propose three repetitions, interleaved in a fixed recorded case order as today.
Report median/min/max and every trial. No selective retries, dropped outliers,
confidence intervals or p95 claims from three observations. One repetition is a
smoke result and cannot receive a comparative pass. Investigate noisy runs with
a separate complete repeated acquisition; preserve the original outcome.

Start with existing latency allowances: both 20% and 100 ms for paint, both 15%
and 200 ms for idle. For a budget crossing, disjoint slower observed ranges mean
regression; overlapping ranges mean inconclusive. Within-budget means only that
these workloads did not cross these review thresholds. It cannot exclude smaller
regressions, rare stalls, or drift hidden by three samples. Calibrate before
making this a gate; do not loosen budgets merely to get green results.

Preserve exit meanings: 0 comparative pass, 1 demonstrated regression, 2 invalid
or inconclusive, with distinct report reasons. A suite deadline produces an
incomplete non-pass, never a partial green report. Recommend a 60-second soft
runtime objective initially, with explicit overrun reporting; a mandatory hard
abort and its cleanup allowance remain a user decision.

## Runtime estimate, not a benchmark result

The selected 15 saved trials total 28.771 seconds launch-to-idle and 282.055
seconds launch-to-last-sample. This excludes process shutdown, artifact writing,
fixture construction, hashing, model copies and builds. The original seed alone
measured 1.614 seconds to idle and 18.483 seconds to its last sample. Timestamps
do not supply a reliable complete command runtime.

| Candidate command component | Budget, seconds | Basis |
| --- | ---: | --- |
| 15 launches through idle | 29.3 | Three times the sum of selected per-case observed maxima, not a statistical bound |
| 15 extra guards | 7.5 | Proposed 0.5 seconds each |
| One seed through idle plus guard | 2.2 | Observed 1.614 seconds plus proposed guard, rounded |
| Fingerprints, model copies, fixture creation, polling overshoot, cleanup and report | 12.0 | Unmeasured allocation, must validate |
| Remaining margin below 60 | 9.0 | About 51 seconds allocated above |

Model hashing reads all model bytes even when copies use APFS clones. Each
1,000-asset fixture creates files and hashes content. A non-APFS copy fallback,
slower machine, scheduling, or delayed window callback can break this estimate.
Fresh process does not mean cold filesystem cache. No claim yet includes
compilation, model acquisition or truly cold setup. See Q1.

## Alternatives and tradeoffs

- Keep the contract and choose one case x3. Even `import-1` needs about 54 seconds
  through collection, before setup. Broad fast coverage requires a new contract.
- Remove holds but keep all 17 x3. About 168 seconds from idle medians alone.
- Run all 17 once without holds. About 56 seconds from idle medians alone, before
  setup or guards, and loses the existing repetition/noise policy.
- Add trickle to the recommended five. Idle medians rise from 28.8 to 45.5
  seconds for 18 trials, leaving too little setup/guard margin. An explicit
  alternative is replacing `board-1000` with trickle, adding about 6.1 seconds
  while losing large Board coverage.
- Use combined 1,000/1,000 instead of separate size cases. Its idle range was
  8.117..24.345 seconds. It covers interactions better but risks the whole budget
  and makes diagnosis less specific. Backfill alone needs about 55.4 seconds x3.
- Shorten CPU/RSS windows. This changes CPU resolution, allocator/helper lifetime
  coverage and retained-memory meaning. It cannot reuse full budgets/baselines.
  Defer instead of publishing misleading resource results.
- Microbenchmarks or one persistent app process. Cheaper but miss real launch,
  helper/model startup and isolation. Parallel app trials contaminate measurements.
  Neither is a substitute for this release-app proposal.

Full remains required for architecture refactors under current repository rules.
Recommend it for release candidates and targeted full-contract cases for changes
affecting omitted workloads or idle/helper/memory behavior. Fast passing never
waives those checks. Any lighter full-suite cadence requires a separate decision.

## Ordered implementation plan, blocked on intent

1. Settle Q1..Q3 and dependent questions. Update this PR's scope and acceptance.
   Put only approved lasting contract facts in the existing perf skill/owning
   evaluator. No new product spec module, ADR, or glossary is currently justified.
2. Reinspect the exact implementation base, especially #201's completion changes.
   Add one profile definition in `scripts/perf_eval.py` for cases, metrics and
   stop/completeness rules. Retain the full contract and separate fast reference.
3. Implement fast collection and seed completion with existing isolation and
   cleanup. Record setup, each trial, cleanup and total monotonic wall time.
   Keep summary validity strict and make incomplete acquisitions unambiguously red.
4. Add a small set of evaluator tests covering contract mismatch, missing metric
   or case/repeat, completion failure/resumed work, timeout cleanup, and the three
   comparison outcomes. Reuse existing full fixtures/tests rather than duplicating
   all app behavior tests. Do not add production timing hooks unless necessary.
5. When the machine is free, acquire a new fast reference and unchanged-binary
   controls. Measure actual command runtime, retained failures and case spread.
   Confirm full-contract behavior still matches the published measurement rules.
6. Publish measured runtime/coverage limits and commands in the existing perf skill.
   Remove this provisional document when its decisions and tasks live in their
   permanent owner and the PR. Do not replace the existing reference baseline.

## Acceptance evidence for later implementation

The task's explicit constraints govern preservation/isolation. Fast budgets and
coverage below are proposed, not newly approved canonical clauses.

| Acceptance | Required evidence |
| --- | --- |
| Full preserved | Existing evaluator tests, unchanged baseline hash, unchanged full contract/case/metric sets, and a later full acquisition reviewed against its matching reference |
| Useful fast detection | Three fresh trials of every selected case with complete paint/work/cardinality evidence; synthetic recorded latency shifts prove pass/regression/inconclusive classification, but do not claim end-to-end sensitivity |
| No false green | Missing clock/summary/metric/case/repeat, timeout, resumed work and incomplete embeddings produce non-pass with retained artifacts; profile mismatch refuses comparison |
| Runtime claim | At least 10 sequential warm-prepared complete commands on the baseline-class Mac, across two idle sessions. Report each total and setup/trial/cleanup durations, min/median/max, failures and overruns. No p95 guarantee from 10 runs |
| Noise calibration | Compare same-binary acquisitions against the new fast reference. Review every false alarm/inconclusive result before enabling a gate; do not silently replace controls or widen budgets |
| End-to-end sensitivity | Later isolated controlled delay in a measured startup/import path above its allowance, then remove it. Show the complete runner detects it. This is future validation, not authorization to edit app code now |
| Setup honesty | Separately measure first prepared run and cold/build/model setup matching Q1's answer. Include fingerprinting, fixture seed/construction, cleanup and reporting in command totals |

Relevant existing spec context: `active-board` operation `activate` behavior
`startup-visible-prepared`, and `image-preparation` operation `request-residency`
behavior `startup-visible-first`. Fast latency checks can provide evidence around
those paths but cannot prove scheduling priority alone. `image-preparation`
guarantee `continuous-interaction-nonblocking` requires interaction evidence,
which this suite does not supply. `semantic-search` guarantees `quiet-idle-backfill`
and `one-backfill-step-at-a-time` remain outside fast coverage. No existing clause
sets a one-minute evaluator budget. The current 13-module spec tree validates;
this PR does not change it.

[PR #201](https://github.com/noelcserepy/snappyref/pull/201) was OPEN at inspected
head `0648682be84570208442016aa1aa3ff6dfbedca2`. Its file list includes
`src/app/perf_scenario.rs` and new `src/diagnostics/import_completion.rs`, so
implementation must recheck marker/completion ownership if it lands first.
There is no planning dependency on #201, and this proposal does not change its
required full evaluation. Do not reuse its running artifacts as fast baselines
or interrupt its processes. Its branch and artifacts were read-only here.

## Unresolved intent, first grilling round

These are independent questions. Recommendations are not answers or approval.

1. What does the one-minute clock include: compilation, model download/install,
   model copying/fingerprints, fixture preparation, and cold setup? Is 60 seconds
   a hard deadline or a normal-run target? Recommendation: normal prepared-run
   target, excluding compilation/download/install, including all per-command
   hashing, copying, fresh fixture seed/construction, trials, cleanup and reporting.
   Report cold setup separately. A cold all-inclusive limit needs a different plan.
2. What decision should a fast result authorize: local feedback, a push gate,
   or merge approval? Recommendation: local feedback initially, with no waiver
   of the existing full refactor checks. Gate adoption waits for noise calibration.
3. Which coverage loss is acceptable to buy the minute: idle CPU/memory, trickle
   reuse, backfill, combined scaling, or large Board latency? Recommendation:
   choose the five latency cases above and keep the other workloads in full.
   If cached-completion hangs matter more than large Board latency, replace
   `board-1000` with trickle and validate the smaller runtime margin.

After these answers, ask only the newly unblocked decisions: acceptable overrun
and retry behavior, tolerated inconclusive frequency if gating is desired,
mandatory targeted/full-run triggers, and which coverage to cut if measured
setup exceeds the allocation. Do not silently sacrifice repetitions or completion
checks to hit the deadline. No implementer/reviewer loop, merge, or implementation
approval belongs to this planning task.
