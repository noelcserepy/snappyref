use std::collections::HashMap;
use std::time::Instant;

use eframe::egui;

use super::{
    AnnotationDraftState, AnnotationDragState, CanvasContextMenuRequest, CanvasCropRequest,
    CropState, DragState, MarqueeState, PendingAnnotation, PendingCropPreview,
    PlacementTransformState, RenderedPlacement, SharedRgba, TextboxLayoutCacheEntry,
    TextboxTransformState,
};
use crate::data::{CanvasAnnotation, CanvasImageLod, CanvasStackItem};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MouseWheelZoomAnimation {
    pub start_pan: egui::Vec2,
    pub start_zoom: f32,
    pub target_pan: egui::Vec2,
    pub target_zoom: f32,
    pub started_at_seconds: f64,
}

#[derive(Default)]
pub(crate) struct CanvasDocumentState {
    pub(crate) rendered_placements: Vec<RenderedPlacement>,
    pub(super) annotations: Vec<CanvasAnnotation>,
    pub(super) stack_order: Vec<CanvasStackItem>,
    pub(super) selected_placement_ids: Vec<i64>,
    pub(super) selected_annotation_ids: Vec<i64>,
    pub(super) dirty_placement_ids: Vec<i64>,
    pub(super) pending_archived_placement_ids: Vec<i64>,
    pub(super) dirty_annotation_ids: Vec<i64>,
    pub(super) pending_archived_annotation_ids: Vec<i64>,
    pub(super) stack_order_dirty: bool,
}

/// The one pointer gesture the canvas is in the middle of. Starting a
/// gesture replaces whatever was active; releasing the primary button ends it.
#[derive(Clone)]
pub(crate) enum ActiveGesture {
    PlacementDrag(DragState),
    AnnotationDrag(AnnotationDragState),
    PlacementTransform(PlacementTransformState),
    TextboxTransform(TextboxTransformState),
    Marquee(MarqueeState),
    Crop(CropState),
    AnnotationDraft(AnnotationDraftState),
}

#[derive(Default)]
pub(crate) struct CanvasInteractionState {
    pub(super) editing_text_annotation_id: Option<i64>,
    pub(super) focus_text_editor: bool,
    pub(crate) gesture: Option<ActiveGesture>,
    pub(super) crop_pending_placement_ids: Vec<i64>,
    pub(super) pending_crop: Option<CanvasCropRequest>,
    pub(super) pending_annotation: Option<PendingAnnotation>,
    pub(super) pending_context_menu_request: Option<CanvasContextMenuRequest>,
    pub(super) pending_focus_selection_request: bool,
    pub(crate) guide_snap_indicators: Vec<super::GuideSnapIndicator>,
    pub(super) guide_snap_signature: u64,
    pub(super) guide_snap_engaged_at: Option<Instant>,
    pub(super) mouse_wheel_zoom_animation: Option<MouseWheelZoomAnimation>,
    pub(super) last_pointer_screen_pos: Option<egui::Pos2>,
    pub(super) alt_pan_active: bool,
}

impl CanvasInteractionState {
    pub(super) fn end_gesture(&mut self) {
        self.gesture = None;
    }

    /// Selection edits end transform gestures but leave pointer captures
    /// (drag, marquee, crop, draft) alone: those edit the selection
    /// themselves while they run.
    pub(super) fn end_transform_gesture(&mut self) {
        if matches!(
            self.gesture,
            Some(
                ActiveGesture::PlacementTransform(_)
                    | ActiveGesture::TextboxTransform(_)
                    | ActiveGesture::AnnotationDrag(_)
            )
        ) {
            self.gesture = None;
        }
    }

    pub(crate) fn placement_drag(&self) -> Option<&DragState> {
        match &self.gesture {
            Some(ActiveGesture::PlacementDrag(drag_state)) => Some(drag_state),
            _ => None,
        }
    }

    pub(crate) fn placement_drag_mut(&mut self) -> Option<&mut DragState> {
        match &mut self.gesture {
            Some(ActiveGesture::PlacementDrag(drag_state)) => Some(drag_state),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct CanvasRenderState {
    pub(super) background_lod_level: i32,
    pub(super) asset_rgba_cache: HashMap<i64, SharedRgba>,
    pub(super) textbox_layout_cache: HashMap<i64, TextboxLayoutCacheEntry>,
    pub(super) pending_image_lod_loads: HashMap<i64, CanvasImageLod>,
    pub(super) pending_crop_previews: HashMap<i64, PendingCropPreview>,
    pub(super) animation_time_override_seconds: Option<f32>,
}
