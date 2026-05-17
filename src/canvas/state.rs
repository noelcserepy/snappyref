use std::collections::HashMap;

use eframe::egui;

use super::{
    AnnotationDraftState, AnnotationDragState, CanvasContextMenuRequest, CanvasCropRequest,
    CanvasImage, CropState, DragState, ImageTransformState, ImportPlaceholder, MarqueeState,
    PendingAnnotation, PendingCropPreview, SharedRgba, TextboxLayoutCacheEntry,
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
pub(super) struct CanvasDocumentState {
    pub images: Vec<CanvasImage>,
    pub annotations: Vec<CanvasAnnotation>,
    pub stack_order: Vec<CanvasStackItem>,
    pub selected_item_ids: Vec<i64>,
    pub selected_annotation_ids: Vec<i64>,
    pub dirty_item_ids: Vec<i64>,
    pub dirty_annotation_ids: Vec<i64>,
    pub pending_archived_annotation_ids: Vec<i64>,
    pub stack_order_dirty: bool,
}

#[derive(Default)]
pub(super) struct CanvasInteractionState {
    pub search_match_item_ids: Vec<i64>,
    pub active_search_item_id: Option<i64>,
    pub editing_text_annotation_id: Option<i64>,
    pub focus_text_editor: bool,
    pub drag_state: Option<DragState>,
    pub annotation_drag_state: Option<AnnotationDragState>,
    pub image_transform_state: Option<ImageTransformState>,
    pub textbox_transform_state: Option<TextboxTransformState>,
    pub marquee_state: Option<MarqueeState>,
    pub crop_state: Option<CropState>,
    pub annotation_draft_state: Option<AnnotationDraftState>,
    pub crop_pending_item_ids: Vec<i64>,
    pub pending_crop: Option<CanvasCropRequest>,
    pub pending_annotation: Option<PendingAnnotation>,
    pub pending_context_menu_request: Option<CanvasContextMenuRequest>,
    pub pending_focus_selection_request: bool,
    pub import_placeholders: Vec<ImportPlaceholder>,
    pub hover_preview_placeholders: Vec<ImportPlaceholder>,
    pub guide_snap_indicators: Vec<super::GuideSnapIndicator>,
    pub mouse_wheel_zoom_animation: Option<MouseWheelZoomAnimation>,
    pub last_pointer_screen_pos: Option<egui::Pos2>,
    pub alt_pan_active: bool,
}

#[derive(Default)]
pub(super) struct CanvasRenderState {
    pub background_lod_level: i32,
    pub asset_rgba_cache: HashMap<i64, SharedRgba>,
    pub textbox_layout_cache: HashMap<i64, TextboxLayoutCacheEntry>,
    pub pending_image_lod_loads: HashMap<i64, CanvasImageLod>,
    pub pending_crop_previews: HashMap<i64, PendingCropPreview>,
    pub animation_time_override_seconds: Option<f32>,
}
