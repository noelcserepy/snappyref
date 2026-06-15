mod annotation;
mod asset;
mod geometry;
mod placement;
mod session;

pub use annotation::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind,
    CanvasBackground, CanvasStrokeWidth, CanvasTextSize,
};
pub use asset::Asset;
pub use geometry::{RectGeometry, SegmentGeometry};
#[allow(unused_imports)]
pub use placement::AssetId;
pub use placement::{
    Board, BoardSummary, CanvasStackItem, ContentResolutionSource, Placement, PlacementContent,
    PlacementId, PlacementLifecycle, PlacementState,
};
pub use session::{
    CanvasImageLod, DEFAULT_CANVAS_IMAGE_MAX_SIDE, DEFAULT_PLACEMENT_GAP,
    DEFAULT_PLACEMENT_TARGET_AREA, DEFAULT_ROTATION_SNAP_DEGREES, DEFAULT_VIEWPORT_ZOOM,
    PointerMode, UserSession, ViewportState, WindowState, WorkspaceSettings,
};
