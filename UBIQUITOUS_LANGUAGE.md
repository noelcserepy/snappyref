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

## Board lifecycle

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **My Board** | The default name for the first board in a library. | Board 1, default scene |
| **Board N** | The generated name for additional boards after the first board. | New board, untitled board |
| **Board Switch** | The operation that saves the active board, clears transient UI state, and loads another board. | Scene change, canvas swap |
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
- Exactly one **Board** is the **Active Board** at a time.
- **This Board** search returns **Placement Results** for the **Active Board**.
- **Library Scope** search returns **Asset Results** from the **Library**.
- **Delete (from board)** removes **Placements** and **Annotations** from the **Active Board** but does not remove **Assets** from the **Library**.
- **Remove from Library** archives **Assets** and removes their **Placements** across all **Boards**.

## Example dialogue

> **Dev:** "When the user imports an image on **My Board**, are we creating an **Asset** or a **Placement**?"
>
> **Domain expert:** "Both: the image enters the **Library** as an **Asset**, and the visible item on **My Board** is a **Placement** of that asset."
>
> **Dev:** "If they press **Delete (from board)**, does the **Asset** disappear from **Library Scope** search?"
>
> **Domain expert:** "No. **Delete (from board)** only removes the **Placement** from the **Active Board**. Use **Remove from Library** when the asset should stop appearing in library search."
>
> **Dev:** "So **This Board** search returns placements, but **Library Scope** search returns assets, including **Unplaced Assets**?"
>
> **Domain expert:** "Exactly. Search scope changes eligibility, not the user's core interaction model."

## Flagged ambiguities

- "Image" was used to mean both **Asset** and **Placement**. Use **Asset** for the shared library object and **Placement** for the board-local instance.
- "Delete" was overloaded between removing board content and removing assets from search. Use **Delete (from board)** for board-local removal and **Remove from Library** for asset archival.
- "Board", "canvas", "scene", "project", and "collection" can blur together. Use **Board** for the user-facing canvas workspace and reserve **Library** for the shared asset pool.
- "Search all photos" should be called **Library Scope** search, not a board named "All Photos" or "Library".
- "Annotation", "note", "textbox", and "drawing" need separation. Use **Annotation** as the umbrella term and **Textbox** for text-bearing annotations.
- "Remove from Library" does not mean **Physical Deletion** in v1. It means **Archived Asset** behavior: removed from boards and search while files are kept for later cleanup.
