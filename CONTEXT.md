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
A board-local canvas item with position, scale, rotation, z-order, archive state, and placement content state. A Placement stores board-local mutable state separately from the library Asset it displays. The asset link is an optional `asset_id`; two Placements may point at the same Asset, and loading or failed Placements may have no loaded Asset object yet. Asset availability changes how the Placement renders, not whether it participates in selection, movement, Guide Snapping, Stack Order, or Document History.
_Avoid_: Board item, image, copy

**Placement State**:
The persisted mutable state inside a Placement: id, optional asset id, Rect Geometry, and archive flag. Placement State is not a separate board object.
_Avoid_: Board item state, item row

**Rect Geometry**:
The canonical geometry primitive for rectangular canvas things: center, size, scale, rotation, and z-order.
_Avoid_: Position fields, transform bag, Placement Footprint, Selected Layout Item

**Placement Display Size**:
The board-world size stored in a Placement's Rect Geometry and used to draw the Placement.
_Avoid_: Asset size, image size, legacy visual-size field

**Asset Pixel Size**:
The intrinsic pixel dimensions of an Asset's source image.
_Avoid_: Placement size, display size

**Segment Geometry**:
The shared primitive for line-like canvas things: start point, end point, and z-order. Line and arrow annotations use Segment Geometry instead of rectangular geometry.
_Avoid_: Line bounds, endpoint fields

**Placement Content State**:
The asset-resolution state of a Placement. Loaded Placements render their Asset; loading Placements render a placeholder of the known or fallback size while import/search work resolves; failed Placements render an error placeholder but remain normal selectable, movable canvas items.
_Avoid_: Placeholder item, pending item, preview object

**Annotation**:
Board-local non-asset markup such as a textbox, arrow, or line. Textbox annotations use Rect Geometry; line-like annotations use Segment Geometry.
_Avoid_: Note, drawing, mark

**Textbox**:
An annotation containing user-entered text on a board.
_Avoid_: Note, label

**Canvas Background**:
The board-local visual guide mode for the canvas.
_Avoid_: Grid setting, workspace background

**Guide Snapping**:
Manual drag alignment that nudges a moving, resizing, or Scale Drag placement's visual axis-aligned bounds to contact-local placement edges, centers, or gap spacing targets and shows alignment guides. Gap spacing targets align opposite placement edges at the Workspace Settings image gap, while same-axis edge alignment still uses real placement visual bounds. A placement is contact-local on an axis when its opposite-edge gap spacing target is within the active snap threshold and its interval on the other axis overlaps or nearly overlaps the dragged placement.
_Avoid_: Grid snapping, layout snapping, arrangement snapping

**Scale Drag**:
A modifier primary-drag that scales the Placement under the pointer, or all selected Placements when the drag starts on a selected Placement, around the selected Placement group center.
_Avoid_: Image zoom, canvas zoom, resize tool

**Placement Movement**:
The shared Placement transform flow for selected or newly created Placements. It owns selected Placement drag start/update state, Guide Snapping during movement, movement timestamps used by the Selection Border, and transform mutation for all Placement Content States and lifecycle states.
_Avoid_: Search drag preview, placeholder movement, import movement path

**Source Intent**:
A pre-Placement request to create canvas content from an external file, URL, clipboard payload, Search result, existing Asset, or existing Placement. A Source Intent describes what content should be created and any source metadata needed for resolution; it is not a canvas object, has no Placement identity, and does not participate in movement, selection, Stack Order, persistence, or Document History.
_Avoid_: PlacementDraft, pending Placement, placeholder object

**Placement Creation**:
The shared domain flow that turns Source Intents into real Placements immediately. It chooses initial visual dimensions and layout, creates Placements with the correct Placement Content State and lifecycle state, selects them when appropriate, and emits any content-resolution work for unresolved sources. After creation, Placement Movement owns movement and Content Resolution owns async loading.
_Avoid_: Add-to-Canvas module, Search placement path, import placement path

**Placement Lifecycle**:
The shared flow for provisional-vs-committed Placement behavior. A canvas has at most one active provisional Placement batch. Provisional Placements are real canvas content but not durable board content; committed Placements are durable board content. Committing a provisional Placement preserves its Placement identity while assigning durable persistence identity as needed.
_Avoid_: Search placement mode internals, provisional cleanup helper, pending draft lifecycle

**Provisional Placement Selection**:
The temporary active selection used while provisional Placements are being positioned. It may drive Placement Movement and Guide Snapping, but it is not durable user selection until the provisional Placements are committed.
_Avoid_: Preview selection, hover selection

**Content Resolution**:
The async content-loading flow for Placements whose content is not loaded yet. It owns runtime queue id to Placement id correlation, source-specific worker request construction, completion/failure correlation, and cleanup for pending resolution work. Content Resolution may change a Placement's content state from loading to loaded or failed. When true Asset Pixel Size arrives, it may correct Placement Display Size to match the asset aspect ratio while preserving area; Placement Movement still owns authored position changes.
_Avoid_: Import reservation, placeholder completion, worker-owned Placement state

**Content Resolution Source**:
The persisted source descriptor used to retry, finish, or fail a Placement's content resolution, such as an external path, external URL, clipboard payload, or existing Asset. It belongs to Placement Content State after a Placement exists; it is not a pre-Placement Source Intent.
_Avoid_: Placement Content Intent, Source Intent

**Selection Border**:
The outline that shows the current canvas selection; it hides while a placement is moving and reappears after the movement pauses or the drag ends.
_Avoid_: Drag guide, snap outline

**Viewport**:
The board-local pan and zoom state.
_Avoid_: Camera, view, canvas position

**External Drag Intake**:
The normalization of OS/window drag-and-drop input into external asset sources plus an optional canvas screen anchor before Placement Creation.
_Avoid_: Import pipeline, native drop bridge, placement flow

**Placement Batch Anchor**:
The board-world point that records where a batch of new Placements was initially created.
_Avoid_: Import Batch Anchor, placeholder center, drop position

**Placement Target Area**:
The desired board-world display area used by Placement Creation when sizing new Placements from Asset Pixel Size or source dimensions.
_Avoid_: Import target area, image area cap

**Add-to-Canvas**:
User-facing wording for commands that add content to the Active Board. Architecture should use Placement Creation for the domain flow and Source Intent for pre-Placement input. Add-to-Canvas should not become a separate deep Module name.
_Avoid_: Placement Creation, Source Intent, PlacementDraft

**Dense Compact Layout**:
The compact arrangement objective for selected placements: minimize visible holes first, then prefer a rough square when density is close. Squareness should decide only when candidate occupancy is roughly within three percentage points. Compact arrangement is placement-only: it may reorder selected placements and change their positions, but it preserves authored scale and rotation. The packed block should keep clean top and left edges; right-edge cleanliness is only a soft tie-break among near-equivalent candidates.
_Avoid_: Perfect square layout, masonry layout

**Compact Size Normalization**:
The optional pre-packing resize phase of compact arrangement where selected Placements with near-matching visual width or height may be scaled so their visible footprints form cleaner rows or columns.
_Avoid_: Normalize close heights/widths, bucket resize, pre-packing resize

**Placement Footprint**:
Deprecated geometry term. Use **Rect Geometry** for rectangular canvas geometry and keep layout-only adapter types out of domain language.
_Avoid_: Selected Layout Item, layout footprint

**Directional Settle Arrange**:
An arrange command where selected Placements move as far as possible toward one edge of the current selection bounds, stopping at that edge or at the Workspace Settings image gap from already-settled selected Placements.
_Avoid_: Edge justify, arrow-key align, gravity arrange

### Session and preferences

**User Session**:
The persisted state that restores the user's active board, viewport, workspace settings, and sidebar state.
_Avoid_: Preferences row, app state

**Workspace Settings**:
Global user preferences that apply across boards, such as layout gap, image sizing, rotation snap, and textbox defaults.
_Avoid_: Board settings, options

**Always on Top**:
The Workspace Settings window mode that keeps SnappyRef above normal app windows while the user works elsewhere.
_Avoid_: Pin window, floating app

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

**Semantic Search Settings**:
The Settings Tab controls that show Semantic Search install status and let the user install, retry, or remove the local search assets.
_Avoid_: Model settings, AI settings

**Session Dirty State**:
The app's knowledge that current user session state differs from the last saved session.
_Avoid_: Unsaved preferences, dirty app state

**Document Dirty State**:
The app's knowledge that the Active Board's persisted canvas document differs from the in-memory Placements, Annotations, archive state, or Stack Order.
_Avoid_: Unsaved canvas, changed items

**Document History**:
The active-board undo/redo stack of board document edits. It stores board-local changes such as Placement and Annotation creation/removal, transforms, arrange, stack-order changes, crop/relink operations, and board lifecycle edits so they can be reversed in place without reloading the board.
_Avoid_: Crop history, app history, global history

**History Entry**:
A single reversible **Document History** step that captures enough before/after board document state to apply Undo or Redo.
_Avoid_: Command log, event, audit record

**Persistence Queue**:
The off-main-thread handoff for saving User Session and Document Dirty State to the Library without blocking interaction frames. UI code enqueues value snapshots or document changes; explicit flush seams wait for durability when lifecycle correctness requires it.
_Avoid_: Autosave thread, background database

**Flush Seam**:
A lifecycle point where queued persistence must be durable before proceeding, such as board switch, app exit, update restart, or destructive board/library actions.
_Avoid_: Save point, sync barrier

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
Opt-in local image retrieval based on fast local visual/text embeddings, available after the required local search assets are installed.
_Avoid_: AI search, smart search, model download

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

**Image Search Pivot**:
The Asset used as image context for an Active Search. An Image Search Pivot is asset-backed; it may originate from a selected Placement, but the Active Search does not rely on that Placement after launch.
_Avoid_: Placement pivot, pivot placement, reverse image search object

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
The on-disk directory at `<data_root>/models/` that holds the semantic search model files, kept outside the app bundle and beside `library/` so it survives app updates and reinstalls. `data_root` is the portable install folder when that folder is writable; protected installs fall back to the user's app-data directory.
_Avoid_: Bundled model, model cache, model resources

**Image Preparation Helper Process**:
A short-lived child process that decodes managed originals or canvas cache sources, generates **Canvas Image Cache** derivatives, and exits so large decoder/encoder allocator pages do not remain resident in the main app process.
_Avoid_: Permanent image worker, renderer process

**Library-wide Search**:
Semantic Search always returns non-archived Assets from the shared Library, including unplaced Assets and Assets placed on any Board.
_Avoid_: Search Scope, This Board search, Library Scope search, board search

**Asset Result**:
A search result backed by an Asset. Adding an Asset Result to the Active Board creates a new Placement for that Asset. Search results do not highlight or reuse existing Placements on the canvas.
_Avoid_: Placement Result, board result, item result, search match

**Search Result Asset Entry**:
The search adapter row grouped around one Asset. It is a UI/service container, not a domain object.
_Avoid_: Search result group, item result group, scoped result entry

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

**Rendered Placement**:
The canvas render adapter that pairs a Placement with runtime texture, LOD, and animation state. It is UI/render-owned and must not be treated as the domain Placement itself.
_Avoid_: CanvasImage as Placement identity, image record

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
The operation that writes selected board content to the clipboard. SnappyRef always includes its internal metadata payload when possible; when exactly one supported Placement is selected it may also include an OS-native raster image for external image editors.
_Avoid_: Export

**Paste**:
The operation that creates copied or external clipboard content on the active board using placement logic. SnappyRef-native clipboard payloads recreate board content; external image/file/PureRef clipboard payloads enter through **External Drag Intake** and preserve the intended **Placement Batch Anchor**.
_Avoid_: Drop

**PureRef Clipboard Paste**:
The Windows paste path that reads PureRef's clipboard payload, extracts image source paths or URLs, and imports them as a selected batch on the active board.
_Avoid_: PureRef import mode, PureRef sync

**Undo**:
The command that applies the previous **Document History** entry to the Active Board in place.
_Avoid_: Reload board, undo crop only

**Redo**:
The command that reapplies the next **Document History** entry to the Active Board in place after Undo.
_Avoid_: Repeat command

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
The command that starts image-pivot Semantic Search from the Asset displayed by one selected Placement. The selected Placement is only the launch source; the Active Search uses the Asset as its pivot and does not rely on the Placement afterward.
_Avoid_: Reverse image search, placement-pivot search

**Delete (from board)**:
The command that removes selected board-local content from the active board.
_Avoid_: Delete image, remove asset

**Remove from Library**:
The confirmed command that removes selected assets from boards and library search without physically deleting files in v1.
_Avoid_: Permanent delete, delete file

**Import Placeholder**:
Deprecated term for the visual rendering of loading or provisional import content. Visible board content should be represented as a **Placement** with **Placement Content State** and **Placement Lifecycle**, not as a separate placeholder object.
_Avoid_: Temporary item, pending asset, placeholder item

**PlacementDraft**:
Deprecated planning term. Do not introduce a domain object named PlacementDraft. Before creation, content is a Source Intent; after creation, it is a Placement in a lifecycle and content state. Any intermediate layout calculation should stay a private implementation detail of Placement Creation.
_Avoid_: Planned Placement, provisional draft, pending Placement

**Selected Layout Item**:
Deprecated layout adapter term. Use **Rect Geometry** for rectangular canvas geometry and **Placement** when referring to real board content.
_Avoid_: Placement Footprint, layout item

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
The legacy sidebar widget that surfaced an **Available Update** before update controls moved into Settings.
_Avoid_: Update badge, update banner, update toast

**Update Popover**:
The legacy panel anchored to the **Update Pill** before update controls moved into Settings.
_Avoid_: Update modal, update dialog

**Pending Update Descriptor**:
The JSON record the running app writes into `<install_root>/updates/pending-update.json` right before exiting for a **Restart Into Update**. It hands the **Updater Helper** or Windows self-replace runner the parent PID, the current and staged bundle paths, the staging directory, the expected new version, and the original launch args.
_Avoid_: Update manifest (that is the signed `latest.json`), restart token

**Updater Helper**:
The macOS `snappyref-updater` binary that ships inside `SnappyRef.app`. The app launches it during a macOS **Restart Into Update**; the helper waits for the app's PID to exit, swaps the staged app bundle into place, relaunches the new app with the original launch args, and removes the staging directory.
_Avoid_: Update agent, install agent

**Windows Self-Replace**:
The Windows **Restart Into Update** apply path where the running `snappyref.exe` copies itself to a temporary runner in `<install_root>/updates/`, launches that runner with the **Pending Update Descriptor**, and exits. The runner copies the staged executable to a `.new` slot, renames the current executable to `snappyref.exe.old`, renames `.new` into the live slot, launches the new executable, and removes staging.
_Avoid_: Windows updater helper, installer

**Restart Into Update**:
The flow that runs when the user clicks Restart for a staged app update: save the **User Session**, stop worker child processes, then apply the staged update. macOS writes a **Pending Update Descriptor**, launches the **Updater Helper**, and exits. Windows writes a **Pending Update Descriptor**, launches a temporary **Windows Self-Replace** runner, and exits.
_Avoid_: Update install, apply update

### Release operations

**Full Release Workflow**:
The GitHub Actions workflow that builds macOS and Windows app artifacts, packages app-only zips, signs `latest.json`, and publishes the release assets.
_Avoid_: CI, build job

**Semantic Search Release Asset**:
The long-lived standalone `snappyref-semantic-search.zip` hosted on `snappyref-dist` that contains the local assets required to enable **Semantic Search**.
_Avoid_: Full install zip, model artifact, AI package

**Release Build Cache**:
The bounded GitHub Actions cache family used only by release app builds to
speed repeated macOS and Windows builds without letting cache entries grow
unbounded. At most the latest cache per platform/build-profile family should
remain after a full release run.
_Avoid_: CI cache, artifact, release asset

**Release Artifact**:
A transient GitHub Actions artifact passed between release workflow jobs, such
as a built app archive or model bundle. Release artifacts are not the durable
public release assets hosted on `snappyref-dist`.
_Avoid_: Release asset, build cache

## Relationships

- A **Library** has one or more non-archived **Boards**.
- A **Library** has zero or more non-archived **Assets**.
- A **Board** belongs to exactly one **Library**.
- An **Asset** belongs to exactly one **Library**.
- A **Placement** belongs to exactly one **Board** and has one **Placement Content State**.
- A loaded **Placement** references exactly one **Asset**.
- A loading or failed **Placement** may not yet reference an **Asset**.
- A **Content Resolution Source** belongs to a loading or failed **Placement Content State**; a **Source Intent** exists only before **Placement Creation**.
- A **Source Intent** is not a **Placement** and does not belong to **Stack Order** or **Document History**.
- **Placement Creation** turns **Source Intents** into **Placements** immediately; there is no domain-level **PlacementDraft** between them.
- **Placement Lifecycle** owns when provisional Placements become committed document content or are discarded history-neutral.
- Improved source facts may refine provisional **Placement** geometry, but must not replace the active provisional **Placement** batch.
- A **Placement Display Size** should match its **Asset Pixel Size** aspect ratio while preserving display area as much as possible.
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
- **Library-wide Search** returns **Asset Results** from the **Library**.
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
- **Rendered Placement** belongs to the canvas render layer and wraps a **Placement** with runtime image resources; it is not the domain object.
- **Placement Footprint** belongs to transient arrangement/preview calculations and is not persisted board content.
- Storage table and column names such as `board_items`, `item_id`, and `content_resolution_intent` are legacy persistence vocabulary. Keep them inside storage/migration adapters and translate to **Placement**, **PlacementId**, and **Content Resolution Source** at domain boundaries. Use `item_id` in domain code only when the union is truly a **Canvas Item**.

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
> **Dev:** "If they press **Delete (from board)**, does the **Asset** disappear from **Library-wide Search**?"
>
> **Domain expert:** "No. **Delete (from board)** only removes board-local **Canvas Items** from the **Active Board**. Use **Remove from Library** when the asset should stop appearing in Search."
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
- "Search all photos" should be called **Library-wide Search**, not a board named "All Photos" or "Library".
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
