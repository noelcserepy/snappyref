# SnappyRef

SnappyRef is a local-first reference-board app for collecting, arranging, annotating, and searching image references across boards.

## Language

### Core canvas model

**Library**:
The shared pool of non-archived image assets available across all boards.
_Avoid_: All photos view, global board

**Asset**:
An image stored in the library, independent of any board placement.
_Avoid_: Image file, photo, source image

**Board**:
A separate canvas workspace over the shared library.
_Avoid_: Scene, project, collection, nested board

**Active Board**:
The one board currently loaded for interaction.
_Avoid_: Current canvas, open scene

**Placement**:
A board-local instance of an asset with position, scale, rotation, z-order, and archive state.
_Avoid_: Board item, image, copy

**Annotation**:
Board-local non-asset markup such as a textbox, arrow, or line.
_Avoid_: Note, drawing, mark

**Textbox**:
An annotation containing user-entered text on a board.
_Avoid_: Note, label

**Canvas Background**:
The board-local visual guide mode for the canvas.
_Avoid_: Grid setting, workspace background

**Viewport**:
The board-local pan and zoom state.
_Avoid_: Camera, view, canvas position

### Session and preferences

**User Session**:
The persisted state that restores the user's active board, viewport, workspace settings, and sidebar state.
_Avoid_: Preferences row, app state

**Workspace Settings**:
Global user preferences that apply across boards, such as import gap, image sizing, rotation snap, and textbox defaults.
_Avoid_: Board settings, options

**Sidebar State**:
The persisted open/closed state and selected tab of the sidebar.
_Avoid_: Sidebar settings, panel state

**Sidebar Tab**:
One of the sidebar's top-level modes, such as Boards or Settings.
_Avoid_: Sidebar page, sidebar mode

**Boards Tab**:
The sidebar tab for creating, switching, renaming, and deleting boards.
_Avoid_: Board list, board browser

**Settings Tab**:
The sidebar tab for workspace settings and shortcuts.
_Avoid_: Options tab, preferences tab

**Session Dirty State**:
The app's knowledge that current user session state differs from the last saved session.
_Avoid_: Unsaved preferences, dirty app state

**Pointer Mode**:
The workspace setting that selects how the canvas interprets scroll and primary-drag input for navigation.
_Avoid_: Input mode, scroll mode, mouse setting

**Mouse Mode**:
The pointer mode where the mouse wheel zooms around the pointer and `Alt`/`Option`+LMB pans the canvas. The default on all platforms.
_Avoid_: Wheel zoom mode, PC mode

**Trackpad Mode**:
The pointer mode where pinch zooms and two-finger swipe pans, matching native trackpad gestures. `Alt`/`Option`+LMB pan is also available as a fallback.
_Avoid_: Gesture mode, Mac mode

### Layering

**Canvas Item**:
A board-local visible object that participates in selection, deletion, copy/paste, and stack ordering.
_Avoid_: Thing, element, object

**Stack Order**:
The unified front-to-back ordering of placements and annotations on a board.
_Avoid_: Layer order, draw order, z-height

**Front**:
The highest position in a board's stack order.
_Avoid_: Top, above everything

**Back**:
The lowest position in a board's stack order.
_Avoid_: Bottom, behind everything

**Move to Front**:
The command that moves selected canvas items to the front while preserving their relative stack order.
_Avoid_: Bring forward, raise

**Move to Back**:
The command that moves selected canvas items to the back while preserving their relative stack order.
_Avoid_: Send backward, lower

**Relative Stack Order**:
The order selected canvas items have among themselves before a stack-order command or copy/paste operation.
_Avoid_: Internal order, selection order

### Search and context

**Semantic Search**:
Always-on image retrieval based on fast local visual/text embeddings.
_Avoid_: AI search, smart search

**Embedding Backfill**:
Opportunistic background work that creates missing semantic-search embeddings after the app is already usable.
_Avoid_: Startup indexing, image-cache backfill

**Active Board Embedding Backfill**:
The first phase of **Embedding Backfill**, covering assets placed on the **Active Board** before broader library assets.
_Avoid_: Visible preload, board search indexing

**Demand Embedding**:
Semantic-search embedding work requested by an explicit user action, such as search, image-pivot search, import, or crop.
_Avoid_: Backfill, preload

**Retryable Embedding Failure**:
A semantic-search embedding failure that may resolve without changing the asset.
_Avoid_: Permanent failure, bad image

**Terminal Embedding Failure**:
A semantic-search embedding failure that should not be retried automatically until the asset or embedding source changes.
_Avoid_: Retryable failure, temporary failure

**Embedding Retry**:
A later attempt to index an asset after a retryable embedding failure, scheduled only after never-tried embedding work has drained.
_Avoid_: Immediate retry, startup retry

**Exhausted Embedding Failure**:
A retryable embedding failure that has consumed its automatic retry budget and should stay quiet unless demand embedding retries it.
_Avoid_: Startup retry, background retry

**Active Search**:
A semantic-search query or prefetch request currently waiting for results.
_Avoid_: Open search overlay, visible results

**Embedding Source**:
The raster image data used to create a semantic-search embedding; prefer the **Working LOD** when available, with the original asset as fallback.
_Avoid_: Search thumbnail, model image

**Embedding Helper Process**:
A short-lived child process that owns image-side semantic model loading and image embedding for currently queued embedding work, then exits so native model and allocator memory return to the OS. Text semantic-search model loading stays on the demand search path, not the import/backfill image helper path.
_Avoid_: Background model cache, permanent model worker

**Embedding Helper Grace Period**:
The short window during which the embedding worker keeps an idle **Embedding Helper Process** alive after its queue drains, so trickled imports (one drag every couple of seconds) reuse one model load instead of cold-mounting per asset. Default 3s, overridable via `SNAPPYREF_EMBEDDING_HELPER_GRACE_MS`. The helper still exits after the grace window so **Quiescent Memory** returns to baseline.
_Avoid_: Helper keep-alive forever, model cache TTL

**Local Model Store**:
The on-disk directory at `<install_root>/models/` that holds the semantic search model files, kept outside the app bundle and as a sibling of `library/` so it survives app updates and reinstalls.
_Avoid_: Bundled model, model cache, model resources

**Image Preparation Helper Process**:
A short-lived child process that decodes managed originals or canvas cache sources, generates **Canvas Image Cache** derivatives, and exits so large decoder/encoder allocator pages do not remain resident in the main app process.
_Avoid_: Permanent image worker, renderer process

**Search Scope**:
The selected boundary that controls which search results are eligible.
_Avoid_: Filter mode, search mode

**This Board**:
The search scope that returns placements on the active board.
_Avoid_: Board search, current canvas

**Library Scope**:
The search scope that returns non-archived assets from the shared library, including unplaced assets.
_Avoid_: Global search, all photos

**Asset Result**:
A library-scope search result backed by an asset that may have no placement on the active board.
_Avoid_: Unplaced result, library result

**Placement Result**:
A board-scope search result backed by an existing placement on the active board.
_Avoid_: Board result, item result

**Context**:
A meaningful set of images plus optional text or metadata signals used for later retrieval.
_Avoid_: Tag, folder, smart group

**Smart Context Model**:
The later retrieval model that uses boards, spatial clusters, import batches, nearby text, and groups as context signals.
_Avoid_: Auto organization, smart tagging

### Image loading

**Canvas Image Cache**:
The app-owned cached raster derivatives used to draw an asset on the canvas quickly at different levels of detail.
_Avoid_: Thumbnail cache, preview cache

**Canvas Pixel Snapshot**:
A transient CPU-side raster copy of a selected placement's current canvas-sized pixels, used for crop preview and crop undo.
_Avoid_: Canvas image cache, asset cache, permanent preview

**Canvas Image LOD**:
A level of detail for a canvas image cache derivative.
_Avoid_: Cache tier, image mode, downscale mode

**Overview LOD**:
The smallest canvas image LOD used for fast startup and first paint.
_Avoid_: Thumbnail, low-res image, overview tier

**Working LOD**:
The default interactive canvas image LOD used after visible images are promoted from overview quality.
_Avoid_: Downscale, normal preview

**Detail LOD**:
The highest canvas image LOD used only when screen size justifies more resolution.
_Avoid_: Original, full-res, detail tier

**Existing Image Load**:
Loading a persisted placement's asset back onto a board after startup or board switch.
_Avoid_: Startup import, reload

**Backfill**:
Creating a missing canvas image LOD in the background after another LOD has already made the image usable.
_Avoid_: Preload, recache

**Active Animated Image**:
An animated placement whose animation is currently allowed to advance because the placement is selected or hovered.
_Avoid_: Visible animated image, loaded GIF

**Settled Idle**:
The runtime state where visible import work, image residency work, crop work, demand embedding, embedding backfill, and canvas image backfill have no queued or in-flight work.
_Avoid_: Low CPU, quiet process

**Placement Visible**:
The moment an imported or loaded placement first appears on the active board.
_Avoid_: Import done, asset ready

**Import Settled**:
The moment all work directly queued by an import batch has completed, including placement integration, canvas image cache work, demand embedding, and immediate promotion or backfill work for those assets.
_Avoid_: Placement visible, idle

**Time to Idle**:
The headline elapsed time from the beginning of a measured user flow until the app reaches **Settled Idle**.
_Avoid_: CPU idle, import duration

**Idle Tail**:
The first measured period after **Settled Idle**, before CPU and memory readings have necessarily stabilized.
_Avoid_: Quiescent idle, settled idle

**Quiescent Idle**:
The later post-tail measurement window where the app should remain near-zero CPU until user input or new work arrives.
_Avoid_: Settled idle, idle tail

**Quiescent Memory**:
The retained memory footprint measured during **Quiescent Idle**, after import, embedding, backfill, and the immediate post-idle tail have drained. Refers to the main process only; **Embedding Helper Process** and **Image Preparation Helper Process** have exited by this window.
_Avoid_: Peak memory, import memory

**Process Tree Peak Memory**:
The peak summed RSS, across one perf sampling tick, of the main app process plus every helper process descended from it (most importantly the **Embedding Helper Process** and **Image Preparation Helper Process**) during active work. Complements **Quiescent Memory**: a faster pipeline that simply shifts allocations into a helper improves **Quiescent Memory** while raising this number.
_Avoid_: Peak RSS, idle peak

**Time to Quiescent**:
The elapsed time from **Settled Idle** until CPU remains below the idle target for a sustained window.
_Avoid_: Time to idle, import duration

**Promotion**:
Requesting a higher canvas image LOD for a visible image when the current LOD is insufficient.
_Avoid_: Upgrade, reload

**Crop Preview**:
The immediate visual replacement shown while a crop worker creates the persisted cropped asset.
_Avoid_: Final crop, cached crop

### Board lifecycle

**My Board**:
The default name for the first board in a library.
_Avoid_: Board 1, default scene

**Board N**:
The generated name for additional boards after the first board.
_Avoid_: New board, untitled board

**Board Switch**:
The operation that saves the active board, clears transient UI state, and loads another board.
_Avoid_: Scene change, canvas swap

**Active Board Load**:
The runtime operation that makes the app match the Library's current **Active Board** after startup, **Board Switch**, or active **Board Deletion**.
_Avoid_: Board reload, canvas reload

**Last Requested Board**:
The board selected by the most recent board switch request during rapid switching.
_Avoid_: Latest board, final board

**Board Creation**:
The instant creation of a new empty board that becomes active immediately.
_Avoid_: New file, new project

**Board Deletion**:
A confirmed operation that archives a board and removes its board-local annotations while preserving library assets.
_Avoid_: Delete canvas, delete project

**Archived Board**:
A board hidden from v1 UI after deletion.
_Avoid_: Deleted board, trashed board

**Last Board Invariant**:
The rule that a library must always have at least one non-archived board.
_Avoid_: Empty library state

### Commands and interaction

**Copy**:
The operation that writes selected board content to the app's internal clipboard payload.
_Avoid_: Export, system copy

**Paste**:
The operation that creates copied or external clipboard content on the active board using placement logic.
_Avoid_: Import, drop

**Duplicate**:
The operation that creates an offset copy of selected board content on the same board.
_Avoid_: Copy, clone board

**Select All**:
The operation that selects all active-board placements and annotations.
_Avoid_: Select library, select assets

**Context Menu**:
The right-click menu for selected board content.
_Avoid_: Popup menu, image menu

**Search Using Image**:
The command that starts image-pivot semantic search from one selected asset.
_Avoid_: Reverse image search

**Delete (from board)**:
The command that removes selected board-local content from the active board.
_Avoid_: Delete image, remove asset

**Remove from Library**:
The confirmed command that removes selected assets from boards and library search without physically deleting files in v1.
_Avoid_: Permanent delete, delete file

**Import Placeholder**:
A transient board-scoped visual placeholder for in-flight import work.
_Avoid_: Temporary item, pending asset

### Asset lifecycle

**Archived Asset**:
An asset removed from library search and all boards without physically deleting its stored file in v1.
_Avoid_: Deleted file, trashed image

**Unplaced Asset**:
A library asset with no active placements on any board.
_Avoid_: Orphan image, unused file

**Managed Original**:
The app-owned stored original file for an asset.
_Avoid_: Source file, cached image

**Physical Deletion**:
The future operation that permanently deletes stored files for removed assets.
_Avoid_: Remove from library, archive

### App lifecycle and updates

**Available Update**:
A published app version newer than the running one, discovered by the app's background update check.
_Avoid_: Pending update, downloaded update, new release

**Update Pill**:
The sidebar widget that surfaces an **Available Update**. Visible only in the **Available**, **Downloading**, **Ready**, or **Failed** state; the rest of the time it is **Hidden**. Clicking an Available pill opens the **Update Popover**; clicking a Ready pill triggers the **Restart Into Update** flow.
_Avoid_: Update badge, update banner, update toast

**Update Popover**:
The small panel anchored to the **Update Pill** that shows the new version, a release-notes link, an Install button, and a Later button. Install starts the in-app download/verify/stage flow.
_Avoid_: Update modal, update dialog

**Pending Update Descriptor**:
The JSON record the running app writes into `<install_root>/updates/pending-update.json` right before exiting for a **Restart Into Update**. It hands the **Updater Helper** the parent PID, the current and staged bundle paths, the staging directory, the expected new version, and the original launch args.
_Avoid_: Update manifest (that is the signed `latest.json`), restart token

**Updater Helper**:
The `snappyref-updater` binary that ships next to the main `snappyref` executable. The app launches it during a **Restart Into Update**; the helper waits for the app's PID to exit, swaps the staged bundle into place (renaming the prior bundle to `.previous`/`.old`), relaunches the new app with the original launch args, and removes the staging directory.
_Avoid_: Update agent, install agent

**Restart Into Update**:
The flow that runs when the user clicks Restart on a **Ready** **Update Pill**: save the **User Session**, stop worker child processes, write the **Pending Update Descriptor**, launch the **Updater Helper**, and exit the app.
_Avoid_: Update install, apply update

## Relationships

- A **Library** has one or more non-archived **Boards**.
- A **Library** has zero or more non-archived **Assets**.
- A **Board** belongs to exactly one **Library**.
- An **Asset** belongs to exactly one **Library**.
- A **Placement** belongs to exactly one **Board** and exactly one **Asset**.
- An **Asset** can have zero or more **Placements**.
- A **Board** can have zero or more **Placements** and zero or more **Annotations**.
- An **Annotation** belongs to exactly one **Board** and does not belong to the **Library**.
- A **Canvas Item** is either a **Placement** or an **Annotation**.
- A **Board** has one **Stack Order** shared by all of its **Canvas Items**.
- A **Board** has one persisted **Viewport**.
- A **User Session** has exactly one **Active Board**, one **Workspace Settings** value, and one **Sidebar State**.
- **Move to Front** and **Move to Back** preserve **Relative Stack Order** for multi-selection.
- A pasted group preserves its **Relative Stack Order** and is placed at the **Front** of the **Active Board**.
- Exactly one **Board** is the **Active Board** at a time.
- **Active Board Load** reads the **Active Board** from the persisted **User Session**; callers must update the **Library** before invoking it.
- During rapid **Board Switches**, the **Last Requested Board** should become the **Active Board**.
- **This Board** search returns **Placement Results** for the **Active Board**.
- **Library Scope** search returns **Asset Results** from the **Library**.
- An **Existing Image Load** may start at **Overview LOD** and later receive **Promotion** to **Working LOD** or **Detail LOD**.
- **Backfill** creates missing **Canvas Image LODs** without blocking board interaction.
- **Import**, **Promotion**, and **Backfill** may use an **Image Preparation Helper Process** so full-resolution decoder and cache-encoder memory is released when helper work exits.
- An animated placement can be visible without being an **Active Animated Image**; visible-but-inactive animated placements draw their current frame without advancing animation.
- **Placement Visible** can happen before **Import Settled** because embeddings and cache work may still be running after a placement is usable.
- **Demand Embedding** may use an **Embedding Helper Process** so the main app process does not retain semantic model RSS after embedding work drains.
- **Embedding Helper Process** and any in-process embedding work load the semantic model from the **Local Model Store**, which is independent of the app bundle.
- An **Available Update** does not affect the **Local Model Store** or the user-owned `library/`; both are outside the app bundle and untouched by the swap.
- **Time to Idle** is the overall perf profile duration; import and embedding timings explain its phases.
- **Settled Idle** is a work-queue state; low CPU and stable memory are performance targets measured after the app reaches **Settled Idle**.
- **Idle Tail** explains short post-idle cleanup or CPU accounting decay; **Quiescent Idle** is the steady-state CPU target window.
- **Quiescent Memory** is the steady retained-memory target; optimize it before optimizing **Time to Idle** so faster pipelines do not hide retained memory growth.
- **Process Tree Peak Memory** must be reported alongside **Quiescent Memory** because optimizing **Quiescent Memory** by shifting allocations into an **Embedding Helper Process** or **Image Preparation Helper Process** can raise **Process Tree Peak Memory** without changing the main-process number.
- A **Canvas Pixel Snapshot** is warmed on demand for selected placements and should not exist for every loaded placement.
- A **Crop Preview** may use a **Canvas Pixel Snapshot** before the final cropped **Asset** exists.
- **Delete (from board)** removes **Placements** and **Annotations** from the **Active Board** but does not remove **Assets** from the **Library**.
- **Remove from Library** archives **Assets** and removes their **Placements** across all **Boards**.

## Perf Corpus

The first representative perf corpus should contain 20 files: 5 JPEG, 5 PNG, 4 WebP, 1 GIF, 1 SVG, 1 TIFF, 1 BMP, 1 AVIF, and 1 JPEG 2000 / JP2. Use the same corpus for the 20-image batch baseline. Use a medium JPEG from that corpus as the first 1-image baseline until per-format single-image runs exist.

Perf runs should preserve image format in timing output so the import, cache, and embedding phases can be grouped by format. The headline baseline remains **Time to Idle** for the full 1-image and 20-image flows, with per-format results used to explain slow phases.

Perf corpus files should be cached locally under the private gitignored perf asset area so repeat runs do not depend on network access. Each cached file should have manifest provenance with source URL, license/status, role, dimensions, byte size, and format. Prefer ethically sourced, permissive/public, stable, low-risk sources such as Wikimedia Commons/public-domain or CC media and official format sample repositories; avoid sketchy downloads and blind scraping.

## Example dialogue

> **Dev:** "When the user imports an image on **My Board**, are we creating an **Asset** or a **Placement**?"
>
> **Domain expert:** "Both: the image enters the **Library** as an **Asset**, and the visible item on **My Board** is a **Placement** of that asset."
>
> **Dev:** "Does that **Placement** share **Stack Order** with **Annotations** like arrows and textboxes?"
>
> **Domain expert:** "Yes. **Placements** and **Annotations** are both **Canvas Items**, so **Move to Front** and **Move to Back** operate on one unified **Stack Order**."
>
> **Dev:** "If a mixed selection is moved or pasted, do we reorder the selected items?"
>
> **Domain expert:** "No. Preserve their **Relative Stack Order**; pasted groups land at the **Front** so new content is visible."
>
> **Dev:** "If they press **Delete (from board)**, does the **Asset** disappear from **Library Scope** search?"
>
> **Domain expert:** "No. **Delete (from board)** only removes board-local **Canvas Items** from the **Active Board**. Use **Remove from Library** when the asset should stop appearing in library search."
>
> **Dev:** "When the app restarts, should it restore the **Settings Tab**, **Viewport**, and active board?"
>
> **Domain expert:** "Yes. Those belong to the **User Session**. **Workspace Settings** are global, while each **Board** keeps its own **Viewport**."
>
> **Dev:** "Should startup load every placement at **Detail LOD**?"
>
> **Domain expert:** "No. An **Existing Image Load** should make the board usable with **Overview LOD** first, then use **Promotion** and **Backfill** for higher LODs."
>
> **Dev:** "Should every loaded placement keep CPU pixels ready for crop?"
>
> **Domain expert:** "No. Keep canvas drawing fast with **Canvas Image Cache** and warm a **Canvas Pixel Snapshot** only when a selected placement needs crop or undo support."

## Flagged ambiguities

- "Image" was used to mean both **Asset** and **Placement**. Use **Asset** for the shared library object and **Placement** for the board-local instance.
- "Delete" was overloaded between removing board content and removing assets from search. Use **Delete (from board)** for board-local removal and **Remove from Library** for asset archival.
- "Board", "canvas", "scene", "project", and "collection" can blur together. Use **Board** for the user-facing canvas workspace and reserve **Library** for the shared asset pool.
- "Search all photos" should be called **Library Scope** search, not a board named "All Photos" or "Library".
- "Annotation", "note", "textbox", and "drawing" need separation. Use **Annotation** as the umbrella term and **Textbox** for text-bearing annotations.
- "Remove from Library" does not mean **Physical Deletion** in v1. It means **Archived Asset** behavior: removed from boards and search while files are kept for later cleanup.
- "Move to front/back" should not be confused with one-step movement. Use **Move to Front** and **Move to Back** for absolute stack extremes; reserve "bring forward" and "send backward" for possible future one-step commands.
- "Z-height", "z-index", "layer order", and "draw order" all describe **Stack Order** in user/domain language. Use implementation terms only in code.
- "Settings" was used for both **Workspace Settings** and the **Settings Tab**. Use **Workspace Settings** for persisted preferences and **Settings Tab** for the sidebar UI location.
- "Downscale" was used as both a user-facing mode and an implementation detail. Use **Canvas Image Cache** and **Canvas Image LOD** terms instead; the obsolete Original/Downscale mode distinction should not be domain language.
- "Tier" was used for canvas image detail levels. Use **Canvas Image LOD**, **Overview LOD**, **Working LOD**, and **Detail LOD** in domain language.
- "Original" should mean the stored **Managed Original**, not a canvas display mode or canvas image LOD.
- "Blocked board switching" can mean either failed activation or slow image visibility. Use **Board Switch** for activation and **Existing Image Load** for image availability.
- "Cache" is overloaded. Use **Canvas Image Cache** for persisted drawable derivatives and **Canvas Pixel Snapshot** for transient CPU pixels warmed for selected editing operations.
