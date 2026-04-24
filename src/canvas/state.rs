use std::collections::HashMap;

use eframe::egui;

use super::{
    AnnotationDraftState, AnnotationDragState, CanvasCropRequest, CanvasImage, CropState,
    DragState, ImageTransformState, ImportPlaceholder, MarqueeState, PendingAnnotation,
    TextboxLayoutCacheEntry, TextboxTransformState,
};
use crate::data::{CanvasAnnotation, CanvasImageCacheTier};

#[derive(Default)]
pub(super) struct CanvasDocumentState {
    pub images: Vec<CanvasImage>,
    pub annotations: Vec<CanvasAnnotation>,
    pub selected_item_ids: Vec<i64>,
    pub selected_annotation_ids: Vec<i64>,
    pub dirty_item_ids: Vec<i64>,
    pub dirty_annotation_ids: Vec<i64>,
    pub pending_archived_annotation_ids: Vec<i64>,
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
    pub import_placeholders: Vec<ImportPlaceholder>,
    pub hover_preview_placeholders: Vec<ImportPlaceholder>,
    pub move_preview_placeholders: Vec<ImportPlaceholder>,
    pub last_pointer_screen_pos: Option<egui::Pos2>,
}

#[derive(Default)]
pub(super) struct CanvasRenderState {
    pub background_lod_level: i32,
    pub asset_rgba_cache: HashMap<i64, Vec<u8>>,
    pub textbox_layout_cache: HashMap<i64, TextboxLayoutCacheEntry>,
    pub pending_image_tier_loads: HashMap<i64, CanvasImageCacheTier>,
    pub animation_time_override_seconds: Option<f32>,
}
