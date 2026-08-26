# Spec Alignment Ledger

Status: 2026-08-25, after owner approval.

This is a review ledger, not a product contract. The module specs own settled behavior. GitHub issues own implementation drift. This file records why each route was chosen so the backfill can be audited without making the specs carry investigation history.

## Evidence reviewed

- 155 Codex sessions available before the review cutoff.
- All 38 pre-review GitHub issues, 110 pull requests, and 433 commits.
- Current implementation and tests.
- Deleted architecture notes 0001 through 0026 and the deleted image-format plan from repository history.
- Three independent alignment reviews. The third review introduced no new material classification.

High-value session anchors:

- `019e27d6-62ee-7073-bc5c-c99f99a02347`, 2026-05-14: Board history, Board lifecycle history, deletion-as-archive, and removal of the crop-only Undo path.
- `019e35ce-5320-7100-82b3-004d1520627d`, 2026-05-17: one-shot zoom-out for provisional Search positioning. Later viewport-neutral language was import-specific, not a reversal of Search behavior.

High-value durable anchors:

- [Document history](https://github.com/noelcserepy/snappyref/blob/93bd3ddf84f1c612f910fe7985863538a19eb38f/docs/adr/0024-document-history.md)
- [Keep interaction frames storage-free](https://github.com/noelcserepy/snappyref/blob/7411e46bd3e747b6401aef763ca5105b4aab9204/docs/adr/0023-keep-interaction-frames-storage-free.md)
- [Loading Placements are Placements](https://github.com/noelcserepy/snappyref/blob/93bd3ddf84f1c612f910fe7985863538a19eb38f/docs/adr/0026-loading-placements-are-placements.md)
- [Bounded embedding failure retries](https://github.com/noelcserepy/snappyref/blob/ca1ab5ce46bb0d1d99bbac7bc281bc56911827ef/docs/adr/0010-bounded-embedding-failure-retries.md)
- [Deepen Placement architecture](https://github.com/noelcserepy/snappyref/pull/128)
- [Move active Board persistence off interaction frames](https://github.com/noelcserepy/snappyref/pull/114)

## Implementation drift routed to issues

| Issue | Divergence | Provenance | Current implementation evidence | Settled contract |
| --- | --- | --- | --- | --- |
| [#149](https://github.com/noelcserepy/snappyref/issues/149) | Undo and Redo use one app-wide history instead of Board-isolated history. | Contrary to explicit May 14 intent; incomplete implementation. | `SnappyRefApp.history`; `active_board::history::DocumentHistory` | Board Document history is isolated per Board. |
| [#150](https://github.com/noelcserepy/snappyref/issues/150) | Undo and Redo synchronously persist restored state. | Contrary to the storage-free interaction boundary. | Undo and Redo integration call durable Placement and annotation restoration before returning. | Undo and Redo change memory first and queue ordered persistence. |
| [#151](https://github.com/noelcserepy/snappyref/issues/151) | Crop has a private Undo stack plus Document History. | Contrary to explicit May 14 intent; legacy path remained. | `CropFlow.undo_stack`; Undo fallback to `undo_last_crop` | Crop has one Board Document history entry and no private fallback. |
| [#152](https://github.com/noelcserepy/snappyref/issues/152) | Accepted Loading Placements enter history as successful workers finish. | Implementation gap in the accepted Loading Placement model. | `create-committed` persists Loading rows; history is recorded during later result integration. | One accepted multi-item batch enters history once at acceptance. |
| [#153](https://github.com/noelcserepy/snappyref/issues/153) | Board create, rename, and delete bypass Document History. | Contrary to explicit May 14 intent; implementation handoff acknowledged omission. | Document history variants cover Placements and annotations, not Board lifecycle rows. | Board lifecycle actions are undoable archival document actions. |
| [#154](https://github.com/noelcserepy/snappyref/issues/154) | One BackfillFailed result exhausts the scheduler. | Contrary to ADR 0010 and PR 67 retry intent. | `BackfillFailed` sets `embedding_backfill.exhausted`; durable storage still records later retry eligibility. | Retry next idle, then after five minutes, then stop until source change or explicit retry. |
| [#90](https://github.com/noelcserepy/snappyref/issues/90) | Large PNG decode is not capped to a canvas-relevant path. | Known implementation gap; existing issue reused. | PNG uses the general full-decode path. | Bounded decode is promised only where a supported decoder exposes scaled or streaming work. |
| [#155](https://github.com/noelcserepy/snappyref/issues/155) | Document History disappears at restart. | Later settled intent said history survives restart; implementation remained v1 memory-only. | `DocumentHistory` stores runtime vectors only. | Committed Board and lifecycle history is durable across restart. |
| [#156](https://github.com/noelcserepy/snappyref/issues/156) | Inactive target Boards skip final accepted-batch layout. | No contrary product decision found; implementation shortcut. | Result integration returns for an inactive target before final group layout. | The target Board receives the same final layout offscreen. |
| [#157](https://github.com/noelcserepy/snappyref/issues/157) | Repeated Board deletion returns an error. | Previously unspecified edge case; owner resolved it here. | Board storage rejects missing or archived identities. | First delete archives; repeated delete returns `missing` without mutation. |
| [#158](https://github.com/noelcserepy/snappyref/issues/158) | Workspace payload version is stored but not enforced. | Previously unspecified compatibility edge; owner resolved it here. | `UserSessionPayload.version` is parsed without a supported-version gate. | Unsupported future payloads fall back as one unit. |
| [#159](https://github.com/noelcserepy/snappyref/issues/159) | Image decode has no explicit EXIF orientation normalization step. | Missing prior intent; owner resolved it here. | Image Preparation reports decoded pixels and dimensions without orientation application. | Normalize supported orientation before dimensions, geometry planning, and crop. |
| [#160](https://github.com/noelcserepy/snappyref/issues/160) | Invalid RectGeometry can cross external and persistence boundaries. | Missing prior intent; owner resolved it here. | Public geometry fields and construction accept arbitrary floats. | Reject non-finite geometry and non-positive size or scale at untrusted boundaries. |
| [#161](https://github.com/noelcserepy/snappyref/issues/161) | First automatic update discovery runs after ten seconds while still busy. | Implementation failsafe with no matching product decision; owner chose strict idle. | `FIRST_CHECK_MAX_DELAY` bypasses scheduler `WaitForSettledIdle`. | First automatic discovery waits strictly for Settled Idle; manual check bypass remains. |
| [#162](https://github.com/noelcserepy/snappyref/issues/162) | macOS promotion failure does not restore the prior app in the same invocation. | Recovery intent existed; macOS implementation remained incomplete. | The helper backs up the installed app, then returns on failed staged promotion. | Attempt immediate restore; retain backup and report recovery-required if restore also fails. |
| [#163](https://github.com/noelcserepy/snappyref/issues/163) | Provisional commit can persist Placements and fail before shared stack order. | Engineering integrity gap, not a product mandate for one SQLite transaction. | Placement commit and stack-order save are separate durable calls. | The logical Board change is failure-atomic; implementation mechanism remains open. |

## Spec lag corrected

| Module | Missing or stale intent now encoded | Primary evidence |
| --- | --- | --- |
| `spatial-layout` | Guide edge, center, gap, hysteresis, and co-winning semantics; rectangle-like Compact; directional Settle packs items independently inside original selection bounds. | Current geometry modules, tests, inspect scenarios, and prior layout sessions. |
| `board-document` | Board naming and archival lifecycle; one mixed-item total stack; relative reorder stability; per-Board view and background; durable Board-isolated history. | Issues 28-41, PR 114, PR 119, ADRs 0023-0024, May 14 session. |
| `active-board` | Ordered restore and switch lifecycle, previous-Board flush, transient reset, Board view restore, and visible-image preparation. | Current active Board coordinator and PR 114. |
| `canvas-interaction` | Mouse and Trackpad navigation meanings; accepted intake is viewport-neutral; provisional Search may zoom out once; engagement-driven animation; clipboard delegation. | Current input, Search placement, render behavior, May 17 session. |
| `placement` | Stack order removed from RectGeometry ownership; geometry boundary validity made explicit. | Current domain model and Board mixed-stack design. |
| `placement-creation` | Target Board continuity, inactive final layout, viewport-neutral accepted intake, and exactly one history entry per accepted batch. | PR 128, ADR 0026, current async correlation flow, owner decisions. |
| `local-library` | Import-time exact reuse, legacy duplicate reconciliation, and deletion-as-archive across all Board references. | PRs 134 and 138, current archive storage, May 14 session. |
| `image-preparation` | SVG vector preservation, animated GIF and WebP timing, crop preview and failure recovery, broad format family, decoder-aware scaling promise, EXIF policy. | Commit 149e609, current SVG and animation tests, existing issue 90, owner decisions. |
| `semantic-search` | One-shot onboarding, shared install controls, Local Model Store migration, quiet idle backfill, and bounded retry schedule. | Issues 96-103, ADRs 0009-0010, PR 67, current model-store migration. |
| `app-update` | Strict idle discovery, six-hour cadence, bounded backoff, manual throttle, quiet failures, user-gated download and restart, data ownership, and recovery outcomes. | Issues 77-84, PR 122, current scheduler and updater helper, owner decisions. |
| `clipboard-exchange` | Internal-first paste precedence, PureRef and native fallbacks, 200 MiB boundary, lossless internal copy, and native single-image export. | Current clipboard and platform adapters plus tests. |
| `local-data-location` | Portable-first shared data root, protected-install fallback, Library-only override, and install-root distinction. | Current location module and release operations guide. |
| `workspace-continuity` | Per-Board view removed from workspace ownership; Mouse and Placement Gap defaults; unsupported future payload fallback remains whole-payload. | Current session payload, Board viewport behavior, owner decisions. |

## Owner decisions resolved during review

| Decision | Route |
| --- | --- |
| One accepted multi-item addition is one Undo entry. | Specs plus [#152](https://github.com/noelcserepy/snappyref/issues/152). |
| Document History survives restart. | Specs plus [#155](https://github.com/noelcserepy/snappyref/issues/155). |
| Inactive completion applies final group layout to its target Board offscreen. | Specs plus [#156](https://github.com/noelcserepy/snappyref/issues/156). |
| Repeated Board deletion returns `missing`. | Specs plus [#157](https://github.com/noelcserepy/snappyref/issues/157). |
| Unsupported future Workspace payloads fall back as one unit. | Specs plus [#158](https://github.com/noelcserepy/snappyref/issues/158). |
| EXIF normalization precedes dimensions and crop. | Specs plus [#159](https://github.com/noelcserepy/snappyref/issues/159). |
| Invalid RectGeometry is rejected at external and persistence boundaries. | Specs plus [#160](https://github.com/noelcserepy/snappyref/issues/160). |
| First automatic update check waits strictly for Settled Idle. | Specs plus [#161](https://github.com/noelcserepy/snappyref/issues/161). |
| Failed macOS promotion restores the prior app in the same invocation. | Specs plus [#162](https://github.com/noelcserepy/snappyref/issues/162). |
| Bounded decode is promised only for formats with a scaled or streaming decoder path. | Spec plus existing [#90](https://github.com/noelcserepy/snappyref/issues/90). |
| Active Board, Clipboard Exchange, and Local Data Location are explicit modules. | New module specs. |

## Remaining missing intent

None in the reviewed discrepancy set. Historical gaps that could not be resolved from old sessions were converted into explicit owner decisions by the approval on 2026-08-25. New behavior discovered outside this evidence cutoff still needs normal intent review rather than automatic spec backfill.
