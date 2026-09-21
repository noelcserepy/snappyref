# Mac continuation, September 21

Known findings addressed. Full functional catalog still incomplete.
Allowance starts at 7%, ceiling 17%. New work stopped at 15%, eight points used. Readings are rounded and account-wide.

## Fixed this round

- Line and arrow endpoints now move independently. The opposite end stays fixed;
  dragging the body still moves the entire segment. Each gesture has one Undo.
- Copied selections survive duplicate-image reconciliation and reopening.
  Storage preserves merged IDs and never gives those IDs to unrelated imports.
- Removing Library images prunes their group membership on every board, including
  cached documents and documents loaded after restarting.
- Moving or copying a closed Library resolves its managed images in the new
  folder. Copying remains independent even while the original folder exists.

Two prior suspected bugs were input problems. The moved-textbox double-click
used one-second test timing; normal timing enters the editor and replacement
text persists. Native group rename passes with confirmed field focus, appears
in the G picker, and is saved on disk after quitting. Native relaunch of that
specific renamed group was not observed.

## Verification

- Full suite: 657 library tests and 7 other tests passed, 3 ignored.
- One additional group-picker test passed after the full suite, with no product edits.
- 23 fresh standard inspect reports passed with assertions and frame files.
- Formatting, compilation, Clippy, scenario-registry tests, release build passed.
- Release image helper and app-only archive validation passed.
- Current release build completed all 15 fast performance trials in 39.38 seconds.
  Median 20-image queue-to-visible time was 576 ms.
- The older saved candidate timed out after 180 seconds on import-20, with 8
  loaded and 12 pending placements. Its report is invalid. No speed comparison
  or absence-of-regression claim follows from the successful current run.
- Full command logs, archive hashes and raw reports remain in the local run directory listed below.

## Remaining checks by category

These are coverage gaps, not confirmed app failures. Earlier passing evidence
applies to unchanged behavior. A passing code assertion does not prove physical
input or real semantic relevance.

| Categories | Remaining |
| --- | --- |
| 1–5 window, boards, intake, images, navigation | Physical wheel/pinch, held-modifier drags, cross-app drop, window stacking/minimization/position. Animated WebP engagement and larger-board image-detail review. |
| 6 transforms/crop | Rotated edge/corner resize, repeated/tiny/boundary crops, complete context-menu routes. |
| 7 arrangement | Duplicate/import at both tested gaps, precise directional endpoints and guide appearance across zoom. |
| 8 groups | Numeric spacing/padding/width, context-menu acceptance, complete member deletion and ungroup routes, intake/board-switch combinations. Remembered destinations, arrow navigation, empty destination, typing and both cancel routes now pass. Native group drag and Redo input still unverified. Rename discrepancy closed. |
| 9 annotations | Native text focus/typing and remaining detailed combinations. Independent endpoints and moved-textbox editing now pass the app harness. |
| 10 clipboard | PureRef unavailable. Remaining mixed-content/native round-trip combinations. Merged-ID failure fixed. |
| 11 model setup | Live download/cancel/retry and removal/reinstall with real assets. Some entry points and confirmation cancellation remain native gaps. |
| 12 search | Real subject relevance and new-subject indexing; full recent-click/restart, pivot clearing, scrolling/paging, drag-back cancellation, small-preview and search-history chains. Existing code covers individual parts. |
| 13 Library | Native cancellation and P invocation, shared-asset deletion followed by search as one chain. Group removal and legacy duplicate storage checks pass. |
| 14 settings/focus | Pointer-mode behavior, Dots appearance, complete shortcut suppression and context-menu combinations. Numeric bounds and invalid text pass. |
| 15 history/persistence | One complete mixed edit/replay chain, divergent redo branch, edit plus view/settings Undo sequence, representative v0.2.0 workspace migration, model availability after root clone. Storage relocation, per-board history, durable group/history and write retry pass. |
| 16 installed/end-to-end | Protected-location isolated account, offline real-model session, complete mixed workflow and timed exploratory session. Portable root selection has code evidence and earlier native restart evidence. |

Unsupported native gestures need a human/device check. PureRef and an old
production workspace need fixtures. Real model download remains excluded under
repo instructions. Updating and Windows remain outside scope.

## Evidence locations

Raw logs, screenshots, fixture data and candidate archives are local, ignored
artifacts under `target/release-testing/2026-09-21-mac/`. They are not included
in this PR. That directory contains `verification.json`, `inspect.json`,
`annotations.json`, `native-rename.json`, `group-picker-navigation.json`,
`code-11-16.json`, and red/green regression logs. Earlier run evidence remains
under `target/release-testing/2026-09-15-mac/` and
`target/release-testing/2026-09-16-mac/`.

The reproducible tests and scenario registry are included in the repository.
See the [functional catalog](release-functional-test-plan.md) and
[execution plan](release-test-execution.md) for scope and test procedures.
No release was published. Only disposable test workspaces were used.
