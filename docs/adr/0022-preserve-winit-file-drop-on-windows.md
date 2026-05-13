# Keep Windows drop target ABI correct

SnappyRef has a Windows native drop bridge so dragged URLs can be extracted from richer shell/browser payloads. That bridge implements `IDropTarget` manually, so its vtable must exactly match the COM ABI. `IDropTarget::DragEnter`, `DragOver`, and `Drop` pass `POINTL` by value. Treating that argument as a pointer is undefined behavior.

The incorrect pointer signature existed while the point was ignored. Once drop placement started using the native drop coordinates, Windows Explorer file drops could fail before import enqueueing.

## Decision

- Keep the Windows `IDropTarget` vtable signatures aligned with the generated Windows bindings: `POINTL` by value, not `*const POINTL`.
- Do not revoke an existing Windows drop target while installing SnappyRef's native drop bridge.
- If `RegisterDragDrop` reports that the window is already registered, keep the existing target and rely on winit for file drops.
- The custom Windows bridge may still register when no prior target exists, but URL-drop support is lower priority than preserving file import.

## Consequences

- Native drop coordinates can be used safely for Windows import placement.
- Explorer file drops keep working through winit if the custom URL bridge cannot attach.
- Browser URL drops on Windows may be unavailable when winit already owns the drop target.
- A future richer URL-drop implementation should chain or cooperate with winit rather than replacing it blindly.
