# Ubiquitous Language

## Core canvas model

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Library** | The shared pool of non-archived image assets available across all boards. | All photos view, global board |
| **Asset** | An image stored in the library, independent of any board placement. | Image file, photo, source image |
| **Board** | A separate canvas workspace over the shared library. | Scene, project, collection, nested board |
| **Active Board** | The one board currently loaded for interaction. | Current canvas, open scene |
| **Placement** | A board-local instance of an asset with position, scale, rotation, z-order, and archive state. | Board item, image, copy |
| **Annotation** | Board-local non-asset markup such as a textbox, arrow, or line. | Note, drawing, mark |
| **Textbox** | An annotation containing user-entered text on a board. | Note, label |
| **Canvas Background** | The board-local visual guide mode for the canvas. | Grid setting, workspace background |
| **Viewport** | The board-local pan and zoom state. | Camera, view, canvas position |

## Session and preferences

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **User Session** | The persisted state that restores the user's active board, viewport, workspace settings, and sidebar state. | Preferences row, app state |
| **Workspace Settings** | Global user preferences that apply across boards, such as import gap, image sizing, rotation snap, semantic search, and textbox defaults. | Board settings, options |
| **Sidebar State** | The persisted open/closed state and selected tab of the sidebar. | Sidebar settings, panel state |
| **Sidebar Tab** | One of the sidebar's top-level modes, such as Boards or Settings. | Sidebar page, sidebar mode |
| **Boards Tab** | The sidebar tab for creating, switching, renaming, and deleting boards. | Board list, board browser |
| **Settings Tab** | The sidebar tab for workspace settings and shortcuts. | Options tab, preferences tab |
| **Session Dirty State** | The app's knowledge that current user session state differs from the last saved session. | Unsaved preferences, dirty app state |

## Layering

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Canvas Item** | A board-local visible object that participates in selection, deletion, copy/paste, and stack ordering. | Thing, element, object |
| **Stack Order** | The unified front-to-back ordering of placements and annotations on a board. | Layer order, draw order, z-height |
| **Front** | The highest position in a board's stack order. | Top, above everything |
| **Back** | The lowest position in a board's stack order. | Bottom, behind everything |
| **Move to Front** | The command that moves selected canvas items to the front while preserving their relative stack order. | Bring forward, raise |
| **Move to Back** | The command that moves selected canvas items to the back while preserving their relative stack order. | Send backward, lower |
| **Relative Stack Order** | The order selected canvas items have among themselves before a stack-order command or copy/paste operation. | Internal order, selection order |

## Search and context

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Semantic Search** | Image retrieval based on visual/text embeddings rather than manual tags. | AI search, smart search |
| **Search Scope** | The selected boundary that controls which search results are eligible. | Filter mode, search mode |
| **This Board** | The search scope that returns placements on the active board. | Board search, current canvas |
| **Library Scope** | The search scope that returns non-archived assets from the shared library, including unplaced assets. | Global search, all photos |
| **Asset Result** | A library-scope search result backed by an asset that may have no placement on the active board. | Unplaced result, library result |
| **Placement Result** | A board-scope search result backed by an existing placement on the active board. | Board result, item result |
| **Context** | A meaningful set of images plus optional text or metadata signals used for later retrieval. | Tag, folder, smart group |
| **Smart Context Model** | The later retrieval model that uses boards, spatial clusters, import batches, nearby text, and groups as context signals. | Auto organization, smart tagging |

## Image loading

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Canvas Image Cache** | The app-owned cached raster derivative used to draw an asset on the canvas quickly. | Thumbnail cache, preview cache |
| **Overview Tier** | The smallest canvas image cache tier used for fast startup and first paint. | Thumbnail, low-res image |
| **Working Tier** | The default interactive canvas image cache tier used after visible images are promoted from overview quality. | Downscale, normal preview |
| **Detail Tier** | The highest canvas image cache tier used only when screen size justifies more resolution. | Original, full-res |
| **Existing Image Load** | Loading a persisted placement's asset back onto a board after startup or board switch. | Startup import, reload |
| **Backfill** | Creating a missing canvas image cache tier in the background after another tier has already made the image usable. | Preload, recache |
| **Promotion** | Requesting a higher cache tier for a visible image when the current tier is insufficient. | Upgrade, reload |

## Board lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **My Board** | The default name for the first board in a library. | Board 1, default scene |
| **Board N** | The generated name for additional boards after the first board. | New board, untitled board |
| **Board Switch** | The operation that saves the active board, clears transient UI state, and loads another board. | Scene change, canvas swap |
| **Active Board Load** | The runtime operation that makes the app match the Library's current **Active Board** after startup, **Board Switch**, or active **Board Deletion**. It clears board-local transient UI state, clears the canvas, loads the **User Session**, restores **Import Placeholders**, and starts **Existing Image Load**. | Board reload, canvas reload |
| **Last Requested Board** | The board selected by the most recent board switch request during rapid switching. | Latest board, final board |
| **Board Creation** | The instant creation of a new empty board that becomes active immediately. | New file, new project |
| **Board Deletion** | A confirmed operation that archives a board and removes its board-local annotations while preserving library assets. | Delete canvas, delete project |
| **Archived Board** | A board hidden from v1 UI after deletion. | Deleted board, trashed board |
| **Last Board Invariant** | The rule that a library must always have at least one non-archived board. | Empty library state |

## Commands and interaction

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Copy** | The operation that writes selected board content to the app's internal clipboard payload. | Export, system copy |
| **Paste** | The operation that creates copied or external clipboard content on the active board using placement logic. | Import, drop |
| **Duplicate** | The operation that creates an offset copy of selected board content on the same board. | Copy, clone board |
| **Select All** | The operation that selects all active-board placements and annotations. | Select library, select assets |
| **Context Menu** | The right-click menu for selected board content. | Popup menu, image menu |
| **Search Using Image** | The command that starts image-pivot semantic search from one selected asset. | Reverse image search |
| **Delete (from board)** | The command that removes selected board-local content from the active board. | Delete image, remove asset |
| **Remove from Library** | The confirmed command that removes selected assets from boards and library search without physically deleting files in v1. | Permanent delete, delete file |
| **Import Placeholder** | A transient board-scoped visual placeholder for in-flight import work. | Temporary item, pending asset |

## Asset lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Archived Asset** | An asset removed from library search and all boards without physically deleting its stored file in v1. | Deleted file, trashed image |
| **Unplaced Asset** | A library asset with no active placements on any board. | Orphan image, unused file |
| **Managed Original** | The app-owned stored original file for an asset. | Source file, cached image |
| **Physical Deletion** | The future operation that permanently deletes stored files for removed assets. | Remove from library, archive |

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
- An **Existing Image Load** may start at **Overview Tier** and later receive **Promotion** to **Working Tier** or **Detail Tier**.
- **Backfill** creates missing **Canvas Image Cache** tiers without blocking board interaction.
- **Delete (from board)** removes **Placements** and **Annotations** from the **Active Board** but does not remove **Assets** from the **Library**.
- **Remove from Library** archives **Assets** and removes their **Placements** across all **Boards**.

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
> **Dev:** "Should startup load every placement at **Detail Tier**?"
>
> **Domain expert:** "No. An **Existing Image Load** should make the board usable with **Overview Tier** first, then use **Promotion** and **Backfill** for higher tiers."

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
- "Downscale" was used as both a user-facing mode and an implementation detail. Use **Canvas Image Cache** and cache-tier terms instead; the obsolete Original/Downscale mode distinction should not be domain language.
- "Original" should mean the stored **Managed Original**, not a canvas display mode or cache tier.
- "Blocked board switching" can mean either failed activation or slow image visibility. Use **Board Switch** for activation and **Existing Image Load** for image availability.
