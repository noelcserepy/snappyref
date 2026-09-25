mod annotation;
mod asset;
mod board;
mod geometry;
mod group;
mod placement;
mod session;

pub use annotation::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind,
    CanvasBackground, CanvasStrokeWidth, CanvasTextSize,
};
pub use asset::Asset;
pub(crate) use geometry::rotated_display_size;
pub use geometry::{RectGeometry, SegmentGeometry};
pub use group::{GapMode, Group, GroupFlow, GroupId, GroupLayoutSettings, MemberLayout};
pub use placement::{
    ContentResolutionSource, Placement, PlacementContent, PlacementId, PlacementLifecycle,
    PlacementState,
};
pub use session::{
    CanvasImageLod, DEFAULT_CANVAS_IMAGE_MAX_SIDE, DEFAULT_PLACEMENT_GAP,
    DEFAULT_PLACEMENT_TARGET_AREA, DEFAULT_ROTATION_SNAP_DEGREES, DEFAULT_VIEWPORT_ZOOM,
    PointerMode, UserSession, ViewportState, WindowState, WorkspaceSettings,
};

pub use board::{Board, BoardDocumentSnapshot, BoardSummary, CanvasStackItem};
