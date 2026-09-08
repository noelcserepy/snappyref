pub(crate) mod residency;
use render::images::{CanvasTexture, PendingCropPreview, PlacementImage, RenderedPlacement};
use std::sync::Arc;

mod document;
mod geometry;
mod groups;
pub(crate) mod guide_snap;
mod interaction;
mod render;
mod state;

use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind,
    CanvasBackground, CanvasImageLod, CanvasTextSize, RectGeometry, SegmentGeometry,
};
use crate::placement::arrangement::PlacementArrangementInput;
use eframe::egui;
use interaction::{
    handle_input, handle_viewport_navigation_input, step_mouse_wheel_zoom_animation,
};
use render::{
    annotation_style_toolbar_rect, editing_textbox_toolbar_rect, paint_annotation_draft,
    paint_background, paint_canvas_items, paint_crop_marquee, paint_guide_snap_indicators,
    paint_hover_hairline, paint_marquee, paint_provisional_placement_previews,
    paint_selected_placement_borders, paint_selection_bounds, show_annotation_style_toolbar,
    show_textbox_editor,
};
pub(crate) use state::ActiveGesture;
use state::{CanvasDocumentState, CanvasInteractionState, CanvasRenderState};

pub(crate) type SharedRgba = Arc<[u8]>;

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;
const MIN_IMAGE_WORLD_SIZE: f32 = 24.0;
const IMAGE_CULL_MARGIN_SCREEN: f32 = 128.0;
const ANNOTATION_CULL_MARGIN_SCREEN: f32 = 128.0;
const PLACEMENT_RESIZE_HIT_RADIUS: f32 = 14.0;
const PLACEMENT_ROTATE_HANDLE_OFFSET_SCREEN: f32 = 18.0;
const PLACEMENT_ROTATE_HANDLE_RADIUS_SCREEN: f32 = 7.0;
const PLACEMENT_ROTATE_HANDLE_HIT_RADIUS_SCREEN: f32 = 13.0;
const SELECTION_BORDER_REAPPEAR_SECONDS: f64 = 2.0;
const PLACEMENT_DRAG_THRESHOLD_SCREEN: f32 = 4.0;
const MIN_TEXTBOX_WORLD_SIZE: egui::Vec2 = egui::vec2(240.0, 56.0);
const TEXTBOX_RESIZE_HIT_RADIUS: f32 = 14.0;
const TEXTBOX_DRAG_THRESHOLD_SCREEN: f32 = 4.0;
const TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH: f32 = 80.0;
const TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH: f32 = 120.0;
const TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH: f32 = 44.0;
const TEXTBOX_EDITOR_TOOLBAR_CLEARANCE: f32 = crate::ui::theme::SPACE_2;
const TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN: f32 = crate::ui::theme::SPACE_2;
const TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP: f32 = crate::ui::theme::SPACE_1;
const ANNOTATION_HIT_RADIUS: f32 = 10.0;
pub(crate) const FOCUS_SELECTION_PADDING_SCREEN: f32 = 48.0;
const BACKGROUND_BASE_WORLD_STEP: f32 = 64.0;
const BACKGROUND_MIN_SCREEN_STEP: f32 = 40.0;
const BACKGROUND_MAX_SCREEN_STEP: f32 = 112.0;
const BACKGROUND_MIN_LOD_LEVEL: i32 = -12;
const BACKGROUND_MAX_LOD_LEVEL: i32 = 12;
const IMAGE_DETAIL_PROMOTION_THRESHOLD: f32 = 0.85;

pub struct CanvasState {
    pan: egui::Vec2,
    pub(crate) zoom: f32,
    background: CanvasBackground,
    pub(crate) placement_gap: f32,
    rotation_snap_degrees: f32,
    pointer_mode: crate::data::PointerMode,
    pub(crate) document: CanvasDocumentState,
    pub(crate) interaction: CanvasInteractionState,
    render: CanvasRenderState,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImagePassMetrics {
    pub visible_images: usize,
    /// Loaded images with paintable textures, excluding loading placeholders.
    pub painted_images: usize,
    pub visible_animated_images: usize,
    pub active_animated_images: usize,
    pub fallback_images: usize,
    pub texture_groups: usize,
    pub draw_calls: usize,
    pub build_time_ms: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasFrameMetrics {
    pub image_pass: ImagePassMetrics,
}

#[derive(Clone, Debug, Default)]
pub struct CanvasDocumentChanges {
    pub updated_placements: Vec<crate::data::Placement>,
    pub archived_placement_ids: Vec<i64>,
    pub updated_annotations: Vec<CanvasAnnotation>,
    pub archived_annotation_ids: Vec<i64>,
    pub stack_order: Option<Vec<crate::data::CanvasStackItem>>,
    pub groups: Option<Vec<crate::data::Group>>,
}

impl CanvasDocumentChanges {
    pub fn has_changes(&self) -> bool {
        !self.updated_placements.is_empty()
            || !self.archived_placement_ids.is_empty()
            || !self.updated_annotations.is_empty()
            || !self.archived_annotation_ids.is_empty()
            || self.stack_order.is_some()
            || self.groups.is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GuideSnapIndicator {
    segments: Vec<(egui::Pos2, egui::Pos2)>,
    marks: Vec<egui::Pos2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanvasInputPolicy {
    Full,
    ViewportOnly,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasHit {
    Placement(i64),
    Annotation(i64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasContextMenuRequest {
    pub screen_pos: egui::Pos2,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub struct CanvasImagePreview {
    pub texture_size: egui::Vec2,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
struct AnnotationPaintStats {
    considered: usize,
    drawn: usize,
}

#[derive(Clone)]
pub(crate) struct PlacementScaleStart {
    placement_id: i64,
    geometry: RectGeometry,
    min_scale: f32,
}

impl PlacementScaleStart {
    fn new(
        placement_id: i64,
        center_world: egui::Pos2,
        base_display_size: egui::Vec2,
        start_scale: f32,
        min_scale: f32,
    ) -> Self {
        Self {
            placement_id,
            geometry: RectGeometry::new(center_world, base_display_size, start_scale, 0.0, 0),
            min_scale,
        }
    }

    fn center_world(&self) -> egui::Pos2 {
        self.geometry.center
    }

    fn base_display_size(&self) -> egui::Vec2 {
        self.geometry.size
    }

    fn start_scale(&self) -> f32 {
        self.geometry.scale
    }
}

#[derive(Clone)]
pub(crate) struct DragState {
    pub(crate) pointer_start_world: egui::Pos2,
    pub(crate) starting_items: Vec<PlacementArrangementInput>,
    pub(crate) snap_preview_rect_world: Option<egui::Rect>,
    pub(crate) started_at_seconds: f64,
    pub(crate) last_movement_seconds: f64,
    pub(crate) last_world_delta: egui::Vec2,
    pub(crate) moved: bool,
}

#[derive(Clone)]
pub(crate) struct AnnotationDragState {
    annotation_ids: Vec<i64>,
    pointer_start_world: egui::Pos2,
    pointer_start_screen: egui::Pos2,
    starting_segments: Vec<(i64, SegmentGeometry)>,
}

#[derive(Clone)]
pub(crate) enum PlacementTransformState {
    Resizing {
        placement_id: i64,
        anchor_screen: egui::Pos2,
        start_handle_screen: egui::Pos2,
        start_scale: f32,
    },
    Scaling {
        placement_ids: Vec<i64>,
        pointer_start_screen: egui::Pos2,
        group_center_world: egui::Pos2,
        starting_items: Vec<PlacementScaleStart>,
    },
    Rotating {
        placement_ids: Vec<i64>,
        center_screen: egui::Pos2,
        center_world: egui::Pos2,
        start_pointer_angle: f32,
        starting_items: Vec<(i64, egui::Pos2, f32)>,
    },
}

#[derive(Clone)]
pub(crate) enum TextboxTransformState {
    PendingMove {
        annotation_ids: Vec<i64>,
        pointer_start_world: egui::Pos2,
        pointer_start_screen: egui::Pos2,
        start_rects_world: Vec<(i64, RectGeometry)>,
    },
    Moving {
        annotation_ids: Vec<i64>,
        pointer_start_world: egui::Pos2,
        start_rects_world: Vec<(i64, RectGeometry)>,
    },
    Resizing {
        annotation_id: i64,
        dragged_handle: TextboxResizeHandle,
        anchor_world: egui::Pos2,
        start_rect_world: RectGeometry,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextboxResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlacementResizeHandle {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxPointerRegion {
    Resize(TextboxResizeHandle),
    Text,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextboxLayoutCacheKey {
    text_hash: u64,
    text_size: CanvasTextSize,
    color: CanvasAnnotationColor,
    wrap_width_bits: u32,
    zoom_bits: u32,
}

#[derive(Clone)]
struct TextboxLayoutCacheEntry {
    key: TextboxLayoutCacheKey,
    galley: Arc<egui::Galley>,
    required_world_height: f32,
}

#[derive(Clone)]
pub(crate) struct MarqueeState {
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

#[derive(Clone)]
pub(crate) struct CropState {
    placement_id: i64,
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

#[derive(Clone)]
pub(crate) struct AnnotationDraftState {
    kind: CanvasAnnotationKind,
    geometry: SegmentGeometry,
}

impl AnnotationDraftState {
    fn new(kind: CanvasAnnotationKind, start_world: egui::Pos2) -> Self {
        Self {
            kind,
            geometry: SegmentGeometry::new(start_world, start_world, 0),
        }
    }

    fn start_world(&self) -> egui::Pos2 {
        self.geometry.start
    }

    fn current_world(&self) -> egui::Pos2 {
        self.geometry.end
    }

    fn set_current_world(&mut self, current_world: egui::Pos2) {
        self.geometry.end = current_world;
    }
}

#[derive(Clone, Debug)]
pub struct PendingAnnotation {
    pub kind: CanvasAnnotationKind,
    pub geometry: CanvasAnnotationGeometry,
    pub text: String,
    pub text_size: CanvasTextSize,
}

impl PendingAnnotation {
    fn new(
        kind: CanvasAnnotationKind,
        start_world: egui::Pos2,
        end_world: egui::Pos2,
        text: String,
        text_size: CanvasTextSize,
    ) -> Self {
        Self {
            kind,
            geometry: CanvasAnnotationGeometry::from_kind(kind, start_world, end_world),
            text,
            text_size,
        }
    }

    pub(crate) fn start_world(&self) -> egui::Pos2 {
        self.geometry.start()
    }

    pub(crate) fn end_world(&self) -> egui::Pos2 {
        self.geometry.end()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelCropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct CanvasCropRequest {
    pub placement_id: i64,
    pub source_asset: crate::data::Asset,
    pub crop_rect: PixelCropRect,
    pub crop_offset_local: egui::Vec2,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            background: CanvasBackground::default(),
            placement_gap: crate::data::DEFAULT_PLACEMENT_GAP,
            rotation_snap_degrees: crate::data::DEFAULT_ROTATION_SNAP_DEGREES,
            pointer_mode: crate::data::PointerMode::default(),
            document: CanvasDocumentState::default(),
            interaction: CanvasInteractionState::default(),
            render: CanvasRenderState::default(),
        }
    }
}

impl CanvasState {
    fn world_to_screen(&self, world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        rect.center() + self.pan + world.to_vec2() * self.zoom
    }

    pub fn screen_to_world(&self, screen: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let world = (screen - rect.center() - self.pan) / self.zoom;
        egui::pos2(world.x, world.y)
    }
}

fn merge_ids(existing: &[i64], incoming: &[i64]) -> Vec<i64> {
    let mut merged = existing.to_vec();
    for id in incoming {
        if !merged.contains(id) {
            merged.push(*id);
        }
    }
    merged
}

pub fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    input_rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_lod_promotion: bool,
    input_policy: CanvasInputPolicy,
    debug_labels: bool,
) -> CanvasFrameMetrics {
    let now_seconds = ui.input(|input| input.time);
    if step_mouse_wheel_zoom_animation(state, now_seconds) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(8));
    }
    let toolbar_rect = state
        .show_group_controls(ui, rect)
        .or_else(|| editing_textbox_toolbar_rect(state, rect))
        .or_else(|| annotation_style_toolbar_rect(state, rect));
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    match input_policy {
        CanvasInputPolicy::Full => {
            if state.interaction.group_picker.is_none()
                && !state.handle_group_pointer(ui, rect, input_rect, toolbar_rect)
            {
                let origins = state.group_transform_origins();
                handle_input(ui, rect, input_rect, toolbar_rect, state, &response);
                state.follow_group_member_transforms(origins);
            }
        }
        CanvasInputPolicy::ViewportOnly => {
            state.interaction.end_gesture();
            handle_viewport_navigation_input(ui, rect, input_rect, toolbar_rect, state, &response);
        }
        CanvasInputPolicy::Blocked => {
            state.interaction.end_gesture();
        }
    }
    state.prune_cached_pixels_to_selected_items();

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::ui::theme::BACKGROUND);
    paint_background(&painter, rect, state);
    state.paint_groups(&painter, rect);
    state.show_group_picker(ui.ctx());
    let image_pass = paint_canvas_items(
        &painter,
        ui,
        rect,
        state,
        render_state,
        allow_lod_promotion,
        debug_labels,
    );
    paint_provisional_placement_previews(&painter, rect, state);
    paint_hover_hairline(
        &painter,
        rect,
        state,
        input_policy == CanvasInputPolicy::Full,
    );
    paint_selected_placement_borders(&painter, rect, state);
    paint_selection_bounds(&painter, rect, state);
    paint_guide_snap_indicators(&painter, rect, state);
    paint_crop_marquee(&painter, rect, state);
    paint_annotation_draft(&painter, rect, state);
    paint_marquee(&painter, state);
    show_annotation_style_toolbar(ui, rect, state);
    show_textbox_editor(ui, rect, state);
    CanvasFrameMetrics { image_pass }
}

fn rotation_handle_key_held(ctx: &egui::Context) -> bool {
    !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_down(egui::Key::R))
}

#[cfg(test)]
mod tests;
