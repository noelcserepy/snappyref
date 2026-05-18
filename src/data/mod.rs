mod annotation;
mod asset;
mod geometry;
mod placement;
mod session;

pub use annotation::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind,
    CanvasBackground, CanvasStrokeWidth, CanvasTextSize,
};
pub use asset::ImageAsset;
pub use geometry::{RectGeometry, SegmentGeometry};
#[allow(unused_imports)]
pub use placement::AssetId;
pub use placement::{
    Board, BoardSummary, CanvasStackItem, ContentResolutionSource, Placement, PlacementContent,
    PlacementFootprint, PlacementId, PlacementLifecycle, PlacementState,
};
pub use session::{
    CanvasImageLod, DEFAULT_CANVAS_IMAGE_MAX_SIDE, DEFAULT_IMPORT_GAP,
    DEFAULT_IMPORT_TARGET_CANVAS_AREA, DEFAULT_ROTATION_SNAP_DEGREES, DEFAULT_VIEWPORT_ZOOM,
    PointerMode, UserSession, ViewportState, WindowState, WorkspaceSettings,
};
