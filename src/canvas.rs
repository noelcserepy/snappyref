use std::sync::{Arc, Mutex};

mod document;
mod geometry;
mod interaction;
mod render;
mod state;

use crate::data::{
    BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind,
    CanvasBackground, CanvasImageCacheTier, CanvasTextSize,
};
use eframe::egui;
use interaction::handle_input;
use render::{
    annotation_style_toolbar_rect, editing_textbox_toolbar_rect, paint_annotation_draft,
    paint_background, paint_canvas_items, paint_crop_marquee, paint_import_placeholders,
    paint_marquee, paint_selection_bounds, show_annotation_style_toolbar, show_textbox_editor,
};
use state::{CanvasDocumentState, CanvasInteractionState, CanvasRenderState};

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;
const MIN_IMAGE_WORLD_SIZE: f32 = 24.0;
const IMAGE_CULL_MARGIN_SCREEN: f32 = 128.0;
const ANNOTATION_CULL_MARGIN_SCREEN: f32 = 128.0;
const IMAGE_RESIZE_HIT_RADIUS: f32 = 14.0;
const IMAGE_MOVE_SNAP_MAX_DISTANCE_SCREEN: f32 = 32.0;
const MIN_TEXTBOX_WORLD_SIZE: egui::Vec2 = egui::vec2(240.0, 56.0);
const TEXTBOX_RESIZE_HIT_RADIUS: f32 = 14.0;
const TEXTBOX_DRAG_THRESHOLD_SCREEN: f32 = 4.0;
const TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH: f32 = 80.0;
const TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH: f32 = 120.0;
const TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH: f32 = 44.0;
const TEXTBOX_EDITOR_TOOLBAR_CLEARANCE: f32 = crate::theme::SPACE_2;
const TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN: f32 = crate::theme::SPACE_2;
const TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP: f32 = crate::theme::SPACE_1;
const ANNOTATION_HIT_RADIUS: f32 = 10.0;
const FOCUS_SELECTION_PADDING_SCREEN: f32 = 48.0;
const BACKGROUND_BASE_WORLD_STEP: f32 = 64.0;
const BACKGROUND_MIN_SCREEN_STEP: f32 = 40.0;
const BACKGROUND_MAX_SCREEN_STEP: f32 = 112.0;
const BACKGROUND_MIN_LOD_LEVEL: i32 = -12;
const BACKGROUND_MAX_LOD_LEVEL: i32 = 12;
const IMAGE_DETAIL_PROMOTION_THRESHOLD: f32 = 0.85;

pub struct CanvasState {
    pan: egui::Vec2,
    zoom: f32,
    background: CanvasBackground,
    layout_gap: f32,
    rotation_snap_degrees: f32,
    document: CanvasDocumentState,
    interaction: CanvasInteractionState,
    render: CanvasRenderState,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImagePaintStats {
    pub considered: usize,
    pub drawn: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PaintPerfReport {
    pub legacy_visibility_pass_ms: f64,
    pub current_visibility_pass_ms: f64,
    pub legacy_considered: usize,
    pub legacy_drawn: usize,
    pub current_considered: usize,
    pub current_drawn: usize,
    pub current_texture_groups: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImagePassMetrics {
    pub visible_images: usize,
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
    pub updated_items: Vec<crate::data::BoardItem>,
    pub updated_annotations: Vec<CanvasAnnotation>,
    pub archived_annotation_ids: Vec<i64>,
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

#[derive(Clone, Copy, Debug)]
pub struct CanvasImagePreview {
    pub texture_id: egui::TextureId,
    pub texture_size: egui::Vec2,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
struct AnnotationPaintStats {
    considered: usize,
    drawn: usize,
}

#[derive(Clone)]
struct CanvasImage {
    record: BoardItemWithAsset,
    texture: CanvasTexture,
    loaded_tier: CanvasImageCacheTier,
    animation: Option<AnimatedCanvasImage>,
}

#[derive(Clone)]
enum CanvasTexture {
    Managed(egui::TextureHandle),
    Native(Arc<crate::jobs::NativeTextureHandle>),
    Svg {
        uri: String,
        bytes: Arc<[u8]>,
        size: [usize; 2],
    },
}

impl CanvasTexture {
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Managed(texture) => texture.id(),
            Self::Native(texture) => texture.texture_id(),
            Self::Svg { .. } => egui::TextureId::Managed(0),
        }
    }

    fn size(&self) -> [usize; 2] {
        match self {
            Self::Managed(texture) => texture.size(),
            Self::Native(texture) => texture.size(),
            Self::Svg { size, .. } => *size,
        }
    }
}

#[derive(Clone)]
struct ImagePassBatch {
    texture: Arc<crate::jobs::NativeTextureHandle>,
    instance_range: std::ops::Range<u32>,
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct ImagePassVertex {
    pos: [f32; 2],
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
struct ImagePassInstance {
    min_ndc: [f32; 2],
    max_ndc: [f32; 2],
    min_uv: [f32; 2],
    max_uv: [f32; 2],
}

struct PreparedImagePass {
    instance_buffer: eframe::wgpu::Buffer,
    bind_groups: Vec<eframe::wgpu::BindGroup>,
}

struct ImagePassResources {
    pipeline: eframe::wgpu::RenderPipeline,
    vertex_buffer: eframe::wgpu::Buffer,
    texture_bind_group_layout: eframe::wgpu::BindGroupLayout,
    sampler: eframe::wgpu::Sampler,
}

struct ImagePassCallback {
    target_format: eframe::wgpu::TextureFormat,
    instances: Vec<ImagePassInstance>,
    batches: Vec<ImagePassBatch>,
    prepared: Mutex<Option<PreparedImagePass>>,
}

const IMAGE_PASS_SHADER: &str = r#"
struct VsIn {
    @location(0) unit_pos: vec2<f32>,
    @location(1) min_ndc: vec2<f32>,
    @location(2) max_ndc: vec2<f32>,
    @location(3) min_uv: vec2<f32>,
    @location(4) max_uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var output: VsOut;
    let ndc = mix(input.min_ndc, input.max_ndc, input.unit_pos);
    output.position = vec4<f32>(ndc, 0.0, 1.0);
    output.uv = mix(input.min_uv, input.max_uv, input.unit_pos);
    return output;
}

@group(0) @binding(0) var image_tex: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    return textureSample(image_tex, image_sampler, input.uv);
}
"#;

#[derive(Clone, Debug)]
struct SelectedLayoutItem {
    id: i64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Clone)]
struct DragState {
    pointer_start_world: egui::Pos2,
    starting_items: Vec<SelectedLayoutItem>,
    snap_preview_rect_world: Option<egui::Rect>,
}

#[derive(Clone)]
struct SearchDragPreview {
    images: Vec<CanvasImage>,
    drop_anchor_world: egui::Pos2,
    snap_preview_rect_world: Option<egui::Rect>,
}

#[derive(Clone)]
struct AnnotationDragState {
    annotation_ids: Vec<i64>,
    pointer_start_world: egui::Pos2,
    pointer_start_screen: egui::Pos2,
    starting_points: Vec<(i64, egui::Pos2, egui::Pos2)>,
}

#[derive(Clone, Copy)]
enum ImageTransformState {
    Resizing {
        item_id: i64,
        center_screen: egui::Pos2,
        start_scale: f32,
        start_radius_screen: f32,
    },
    Rotating {
        item_id: i64,
        center_screen: egui::Pos2,
        start_rotation: f32,
    },
}

#[derive(Clone)]
enum TextboxTransformState {
    PendingMove {
        annotation_ids: Vec<i64>,
        pointer_start_world: egui::Pos2,
        pointer_start_screen: egui::Pos2,
        start_rects_world: Vec<(i64, egui::Rect)>,
    },
    Moving {
        annotation_ids: Vec<i64>,
        pointer_start_world: egui::Pos2,
        start_rects_world: Vec<(i64, egui::Rect)>,
    },
    Resizing {
        annotation_id: i64,
        dragged_handle: TextboxResizeHandle,
        anchor_world: egui::Pos2,
        start_rect_world: egui::Rect,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextboxResizeHandle {
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
enum ImageResizeHandle {
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

#[derive(Clone, Debug, PartialEq)]
struct ImportPlaceholder {
    queue_id: u64,
    center_world: egui::Pos2,
    size_world: egui::Vec2,
    label: String,
}

struct MarqueeState {
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

struct CropState {
    item_id: i64,
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

#[derive(Clone, Debug)]
struct AnimatedCanvasFrame {
    rgba: Vec<u8>,
    delay_seconds: f32,
}

#[derive(Clone, Debug)]
struct AnimatedCanvasImage {
    frames: Vec<AnimatedCanvasFrame>,
    width: u32,
    height: u32,
    current_frame: usize,
}

#[derive(Clone)]
struct AnnotationDraftState {
    kind: CanvasAnnotationKind,
    start_world: egui::Pos2,
    current_world: egui::Pos2,
}

#[derive(Clone, Debug)]
pub struct PendingAnnotation {
    pub kind: CanvasAnnotationKind,
    pub start_world: egui::Pos2,
    pub end_world: egui::Pos2,
    pub text: String,
    pub text_size: CanvasTextSize,
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
    pub item_id: i64,
    pub source_asset: crate::data::ImageAsset,
    pub crop_rect: PixelCropRect,
    pub crop_offset_local: egui::Vec2,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            background: CanvasBackground::default(),
            layout_gap: crate::data::DEFAULT_IMPORT_GAP,
            rotation_snap_degrees: crate::data::DEFAULT_ROTATION_SNAP_DEGREES,
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

    pub(crate) fn screen_distance_to_world(&self, screen_distance: f32) -> f32 {
        screen_distance / self.zoom.max(0.01)
    }

    #[cfg(debug_assertions)]
    pub(crate) fn seed_move_snap_preview_for_selected_items(
        &mut self,
        rect: egui::Rect,
        world_delta: egui::Vec2,
    ) -> bool {
        let drag_state = DragState {
            pointer_start_world: egui::Pos2::ZERO,
            starting_items: self.selected_layout_items(),
            snap_preview_rect_world: None,
        };
        let snap_preview_rect =
            interaction::compute_drag_snap_preview(self, rect, &drag_state, world_delta);
        let snap_offset = snap_preview_rect
            .and_then(|snap_rect| {
                interaction::offset_layout_bounds(&drag_state.starting_items, world_delta)
                    .map(|bounds| snap_rect.center() - bounds.center())
            })
            .unwrap_or(egui::Vec2::ZERO);
        for item in &drag_state.starting_items {
            if let Some(image) = self
                .document
                .images
                .iter_mut()
                .find(|image| image.record.item.id == item.id)
            {
                image.record.item.x = item.x + world_delta.x + snap_offset.x;
                image.record.item.y = item.y + world_delta.y + snap_offset.y;
            }
        }
        snap_preview_rect.is_some()
    }
}

fn horizontal_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_height = items.iter().map(|item| item.height).fold(0.0_f32, f32::max);
    let mut x_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(x_cursor + item.width * 0.5, max_height - item.height * 0.5);
        placements.push((item.id, center));
        x_cursor += item.width + gap;
    }

    let total_width = (x_cursor - gap).max(0.0);
    Some((placements, total_width, max_height))
}

fn vertical_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let max_width = items.iter().map(|item| item.width).fold(0.0_f32, f32::max);
    let mut y_cursor = 0.0;
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let center = egui::pos2(item.width * 0.5, y_cursor + item.height * 0.5);
        placements.push((item.id, center));
        y_cursor += item.height + gap;
    }

    let total_height = (y_cursor - gap).max(0.0);
    Some((placements, max_width, total_height))
}

fn best_compact_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut best_layout: Option<(Vec<(i64, egui::Pos2)>, f32, f32, f32, f32, usize)> = None;
    for (candidate_index, bin_width) in compact_candidate_widths(items, gap).into_iter().enumerate()
    {
        let Some((placements, width, height)) = maxrects_pack_layout(items, bin_width, gap) else {
            continue;
        };
        let area = width * height;
        let squareness = width.max(height) / width.min(height).max(1.0);

        match &best_layout {
            Some((_, _, _, best_squareness, best_area, best_index))
                if squareness > *best_squareness
                    || ((squareness - *best_squareness).abs() < 0.001 && area > *best_area)
                    || ((squareness - *best_squareness).abs() < 0.001
                        && (area - *best_area).abs() < 0.001
                        && candidate_index >= *best_index) => {}
            _ => {
                best_layout = Some((placements, width, height, squareness, area, candidate_index));
            }
        }
    }

    best_layout.map(|(placements, width, height, _, _, _)| (placements, width, height))
}

fn compact_candidate_widths(items: &[SelectedLayoutItem], gap: f32) -> Vec<f32> {
    let total_area = items
        .iter()
        .map(|item| (item.width + gap) * (item.height + gap))
        .sum::<f32>();
    let max_width = items
        .iter()
        .map(|item| item.width + gap)
        .fold(0.0_f32, f32::max);
    let ideal_width = total_area.sqrt().max(max_width);
    let mut widths = vec![ideal_width];
    let mut running_width = 0.0_f32;

    for item in items {
        let effective_width = item.width + gap;
        running_width += effective_width;
        widths.push(running_width.max(max_width));
    }

    widths.sort_by(|left, right| left.total_cmp(right));
    widths.dedup_by(|left, right| (*left - *right).abs() < 0.5);
    widths
}

fn maxrects_pack_layout(
    items: &[SelectedLayoutItem],
    bin_width: f32,
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let total_height = items.iter().map(|item| item.height + gap).sum::<f32>().max(
        items
            .iter()
            .map(|item| item.height + gap)
            .fold(0.0_f32, f32::max),
    );
    let mut free_rects = vec![PackRect {
        x: 0.0,
        y: 0.0,
        width: bin_width,
        height: total_height,
    }];
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let padded = padded_item_rect(item, gap);
        if padded.width > bin_width + 0.001 {
            return None;
        }

        let Some(placed) = place_maxrects_item(&mut free_rects, padded) else {
            return None;
        };
        placements.push((
            item.id,
            egui::pos2(placed.x + item.width * 0.5, placed.y + item.height * 0.5),
        ));
    }

    let (layout_width, layout_height) = placed_bounds(items, &placements)?;
    Some((placements, layout_width, layout_height))
}

fn place_maxrects_item(free_rects: &mut Vec<PackRect>, padded: PackRect) -> Option<PackRect> {
    let mut best_candidate: Option<(usize, PackRect, f32, f32)> = None;

    for (index, free) in free_rects.iter().copied().enumerate() {
        if padded.width > free.width + 0.001 || padded.height > free.height + 0.001 {
            continue;
        }

        let short_side_fit = (free.width - padded.width).min(free.height - padded.height);
        let long_side_fit = (free.width - padded.width).max(free.height - padded.height);
        let placed = PackRect {
            x: free.x,
            y: free.y,
            width: padded.width,
            height: padded.height,
        };

        match best_candidate {
            Some((_, best_rect, best_short, best_long))
                if short_side_fit > best_short
                    || ((short_side_fit - best_short).abs() < 0.001
                        && long_side_fit > best_long)
                    || ((short_side_fit - best_short).abs() < 0.001
                        && (long_side_fit - best_long).abs() < 0.001
                        && (placed.y > best_rect.y
                            || ((placed.y - best_rect.y).abs() < 0.001
                                && placed.x >= best_rect.x))) => {}
            _ => best_candidate = Some((index, placed, short_side_fit, long_side_fit)),
        }
    }

    let (candidate_index, placed, _, _) = best_candidate?;
    split_free_rects(free_rects, candidate_index, placed);
    Some(placed)
}

fn split_free_rects(free_rects: &mut Vec<PackRect>, candidate_index: usize, placed: PackRect) {
    let mut next = Vec::with_capacity(free_rects.len() + 4);

    for (index, free) in free_rects.iter().copied().enumerate() {
        if index == candidate_index || rects_intersect(free, placed) {
            next.extend(split_free_rect(free, placed));
        } else {
            next.push(free);
        }
    }

    prune_free_rects(&mut next);
    *free_rects = next;
}

fn split_free_rect(free: PackRect, placed: PackRect) -> Vec<PackRect> {
    if !rects_intersect(free, placed) {
        return vec![free];
    }

    let mut result = Vec::with_capacity(4);

    if placed.y > free.y + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: free.width,
            height: placed.y - free.y,
        });
    }

    if placed.bottom() < free.bottom() - 0.001 {
        result.push(PackRect {
            x: free.x,
            y: placed.bottom(),
            width: free.width,
            height: free.bottom() - placed.bottom(),
        });
    }

    if placed.x > free.x + 0.001 {
        result.push(PackRect {
            x: free.x,
            y: free.y,
            width: placed.x - free.x,
            height: free.height,
        });
    }

    if placed.right() < free.right() - 0.001 {
        result.push(PackRect {
            x: placed.right(),
            y: free.y,
            width: free.right() - placed.right(),
            height: free.height,
        });
    }

    result
}

fn prune_free_rects(free_rects: &mut Vec<PackRect>) {
    free_rects.retain(|rect| rect.width > 0.001 && rect.height > 0.001);

    loop {
        let mut changed = false;

        let mut index = 0;
        while index < free_rects.len() {
            let rect = free_rects[index];
            let contained = free_rects.iter().enumerate().any(|(other_index, other)| {
                other_index != index && rect_contained_in(rect, *other)
            });
            if contained {
                free_rects.remove(index);
                changed = true;
            } else {
                index += 1;
            }
        }

        let mut left_index = 0;
        while left_index < free_rects.len() {
            let mut right_index = left_index + 1;
            while right_index < free_rects.len() {
                if let Some(merged) =
                    merge_adjacent_free_rects(free_rects[left_index], free_rects[right_index])
                {
                    free_rects[left_index] = merged;
                    free_rects.remove(right_index);
                    changed = true;
                } else {
                    right_index += 1;
                }
            }
            left_index += 1;
        }

        if !changed {
            break;
        }
    }
}

fn rects_intersect(left: PackRect, right: PackRect) -> bool {
    left.x < right.right() - 0.001
        && left.right() > right.x + 0.001
        && left.y < right.bottom() - 0.001
        && left.bottom() > right.y + 0.001
}

fn rect_contained_in(inner: PackRect, outer: PackRect) -> bool {
    inner.x >= outer.x - 0.001
        && inner.y >= outer.y - 0.001
        && inner.right() <= outer.right() + 0.001
        && inner.bottom() <= outer.bottom() + 0.001
}

fn merge_adjacent_free_rects(left: PackRect, right: PackRect) -> Option<PackRect> {
    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (left.bottom() - right.y).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width,
            height: left.height + right.height,
        });
    }

    if (left.x - right.x).abs() < 0.001
        && (left.width - right.width).abs() < 0.001
        && (right.bottom() - left.y).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: right.width,
            height: left.height + right.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (left.right() - right.x).abs() < 0.001
    {
        return Some(PackRect {
            x: left.x,
            y: left.y,
            width: left.width + right.width,
            height: left.height,
        });
    }

    if (left.y - right.y).abs() < 0.001
        && (left.height - right.height).abs() < 0.001
        && (right.right() - left.x).abs() < 0.001
    {
        return Some(PackRect {
            x: right.x,
            y: right.y,
            width: left.width + right.width,
            height: right.height,
        });
    }

    None
}

fn padded_item_rect(item: &SelectedLayoutItem, gap: f32) -> PackRect {
    PackRect {
        x: 0.0,
        y: 0.0,
        width: item.width + gap,
        height: item.height + gap,
    }
}

#[cfg(test)]
fn legacy_best_compact_layout(
    items: &[SelectedLayoutItem],
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut best_layout: Option<(Vec<(i64, egui::Pos2)>, f32, f32, f32, f32, usize)> = None;
    for (candidate_index, bin_width) in legacy_compact_candidate_widths(items, gap)
        .into_iter()
        .enumerate()
    {
        let Some((placements, width, height)) = legacy_skyline_pack_layout(items, bin_width, gap)
        else {
            continue;
        };
        let squareness = width.max(height) / width.min(height).max(1.0);
        let area = width * height;

        match &best_layout {
            Some((_, _, _, best_squareness, best_area, best_index))
                if squareness > *best_squareness
                    || ((squareness - *best_squareness).abs() < 0.001 && area > *best_area)
                    || ((squareness - *best_squareness).abs() < 0.001
                        && (area - *best_area).abs() < 0.001
                        && candidate_index >= *best_index) => {}
            _ => {
                best_layout = Some((placements, width, height, squareness, area, candidate_index));
            }
        }
    }

    best_layout.map(|(placements, width, height, _, _, _)| (placements, width, height))
}

#[cfg(test)]
fn legacy_compact_candidate_widths(items: &[SelectedLayoutItem], gap: f32) -> Vec<f32> {
    let total_area = items
        .iter()
        .map(|item| item.width * item.height)
        .sum::<f32>();
    let max_width = items.iter().map(|item| item.width).fold(0.0_f32, f32::max);
    let ideal_width = total_area.sqrt().max(max_width);
    let mut widths = vec![ideal_width];
    let mut running_width = 0.0_f32;

    for (index, item) in items.iter().enumerate() {
        running_width += item.width;
        if index > 0 {
            running_width += gap;
        }
        widths.push(running_width.max(max_width));
    }

    widths.sort_by(|left, right| left.total_cmp(right));
    widths.dedup_by(|left, right| (*left - *right).abs() < 0.5);
    widths
}

#[cfg(test)]
fn legacy_skyline_pack_layout(
    items: &[SelectedLayoutItem],
    bin_width: f32,
    gap: f32,
) -> Option<(Vec<(i64, egui::Pos2)>, f32, f32)> {
    if items.is_empty() {
        return None;
    }

    let mut skyline = vec![LegacySkylineNode {
        x: 0.0,
        y: 0.0,
        width: bin_width,
    }];
    let mut placements = Vec::with_capacity(items.len());

    for item in items {
        let occupied_width = item.width + gap;
        let occupied_height = item.height + gap;
        if occupied_width > bin_width + 0.001 {
            return None;
        }

        let mut best_candidate: Option<(usize, f32, f32)> = None;
        for node_index in 0..skyline.len() {
            let Some(candidate_y) =
                legacy_skyline_candidate_y(&skyline, node_index, occupied_width, bin_width)
            else {
                continue;
            };
            let candidate_x = skyline[node_index].x;
            match best_candidate {
                Some((_, best_y, best_x))
                    if candidate_y > best_y
                        || ((candidate_y - best_y).abs() < 0.001 && candidate_x >= best_x) => {}
                _ => best_candidate = Some((node_index, candidate_y, candidate_x)),
            }
        }

        let Some((node_index, place_y, place_x)) = best_candidate else {
            return None;
        };
        legacy_skyline_insert(
            &mut skyline,
            node_index,
            place_x,
            place_y,
            occupied_width,
            occupied_height,
        );
        placements.push((
            item.id,
            egui::pos2(place_x + item.width * 0.5, place_y + item.height * 0.5),
        ));
    }

    let (layout_width, layout_height) = placed_bounds(items, &placements)?;
    Some((placements, layout_width, layout_height))
}

#[cfg(test)]
fn legacy_skyline_candidate_y(
    skyline: &[LegacySkylineNode],
    start_index: usize,
    occupied_width: f32,
    bin_width: f32,
) -> Option<f32> {
    let start = skyline[start_index];
    if start.x + occupied_width > bin_width + 0.001 {
        return None;
    }

    let mut width_left = occupied_width;
    let mut index = start_index;
    let mut candidate_y = start.y;

    while width_left > 0.0 {
        let node = *skyline.get(index)?;
        candidate_y = candidate_y.max(node.y);
        width_left -= node.width;
        index += 1;
    }

    Some(candidate_y)
}

#[cfg(test)]
fn legacy_skyline_insert(
    skyline: &mut Vec<LegacySkylineNode>,
    node_index: usize,
    x: f32,
    y: f32,
    occupied_width: f32,
    occupied_height: f32,
) {
    let new_y = y + occupied_height;
    skyline.insert(
        node_index,
        LegacySkylineNode {
            x,
            y: new_y,
            width: occupied_width,
        },
    );

    let index = node_index + 1;
    let end_x = x + occupied_width;
    while index < skyline.len() {
        let node = skyline[index];
        if node.x >= end_x {
            break;
        }

        let overlap = end_x - node.x;
        if overlap >= node.width - 0.001 {
            skyline.remove(index);
        } else {
            skyline[index].x += overlap;
            skyline[index].width -= overlap;
            break;
        }
    }

    legacy_merge_skyline_nodes(skyline);
}

#[cfg(test)]
fn legacy_merge_skyline_nodes(skyline: &mut Vec<LegacySkylineNode>) {
    let mut index = 0;
    while index + 1 < skyline.len() {
        if (skyline[index].y - skyline[index + 1].y).abs() < 0.001 {
            skyline[index].width += skyline[index + 1].width;
            skyline.remove(index + 1);
        } else {
            index += 1;
        }
    }
}

fn placed_bounds(
    items: &[SelectedLayoutItem],
    placements: &[(i64, egui::Pos2)],
) -> Option<(f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (item_id, center) in placements {
        let item = items.iter().find(|item| item.id == *item_id)?;
        min_x = min_x.min(center.x - item.width * 0.5);
        min_y = min_y.min(center.y - item.height * 0.5);
        max_x = max_x.max(center.x + item.width * 0.5);
        max_y = max_y.max(center.y + item.height * 0.5);
    }

    Some(((max_x - min_x).max(0.0), (max_y - min_y).max(0.0)))
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

#[derive(Clone, Copy, Debug)]
struct PackRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl PackRect {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct LegacySkylineNode {
    x: f32,
    y: f32,
    width: f32,
}

#[cfg(test)]
pub(crate) fn legacy_compact_layout_area_for_sizes(
    items: &[(i64, f32, f32)],
    gap: f32,
) -> Option<f32> {
    let items = items
        .iter()
        .map(|(id, width, height)| SelectedLayoutItem {
            id: *id,
            x: 0.0,
            y: 0.0,
            width: *width,
            height: *height,
        })
        .collect::<Vec<_>>();
    let (_, width, height) = legacy_best_compact_layout(&items, gap)?;
    Some(width * height)
}

impl ImagePassResources {
    fn new(device: &eframe::wgpu::Device, target_format: eframe::wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(eframe::wgpu::ShaderModuleDescriptor {
            label: Some("reference_canvas_image_shader"),
            source: eframe::wgpu::ShaderSource::Wgsl(IMAGE_PASS_SHADER.into()),
        });
        let texture_bind_group_layout =
            device.create_bind_group_layout(&eframe::wgpu::BindGroupLayoutDescriptor {
                label: Some("reference_canvas_image_bind_group_layout"),
                entries: &[
                    eframe::wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: eframe::wgpu::ShaderStages::FRAGMENT,
                        ty: eframe::wgpu::BindingType::Texture {
                            sample_type: eframe::wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                            view_dimension: eframe::wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    eframe::wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: eframe::wgpu::ShaderStages::FRAGMENT,
                        ty: eframe::wgpu::BindingType::Sampler(
                            eframe::wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    },
                ],
            });
        let sampler = device.create_sampler(&eframe::wgpu::SamplerDescriptor {
            label: Some("reference_canvas_image_sampler"),
            mag_filter: eframe::wgpu::FilterMode::Linear,
            min_filter: eframe::wgpu::FilterMode::Linear,
            mipmap_filter: eframe::wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout =
            device.create_pipeline_layout(&eframe::wgpu::PipelineLayoutDescriptor {
                label: Some("reference_canvas_image_pipeline_layout"),
                bind_group_layouts: &[Some(&texture_bind_group_layout)],
                immediate_size: 0,
            });
        let vertex_attributes = eframe::wgpu::vertex_attr_array![0 => Float32x2];
        let instance_attributes = eframe::wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2];
        let pipeline = device.create_render_pipeline(&eframe::wgpu::RenderPipelineDescriptor {
            label: Some("reference_canvas_image_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: eframe::wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: eframe::wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    eframe::wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ImagePassVertex>() as u64,
                        step_mode: eframe::wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attributes,
                    },
                    eframe::wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<ImagePassInstance>() as u64,
                        step_mode: eframe::wgpu::VertexStepMode::Instance,
                        attributes: &instance_attributes,
                    },
                ],
            },
            primitive: eframe::wgpu::PrimitiveState {
                topology: eframe::wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: eframe::wgpu::MultisampleState::default(),
            fragment: Some(eframe::wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: eframe::wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(eframe::wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(eframe::wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: eframe::wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let vertices = [
            ImagePassVertex { pos: [0.0, 0.0] },
            ImagePassVertex { pos: [1.0, 0.0] },
            ImagePassVertex { pos: [1.0, 1.0] },
            ImagePassVertex { pos: [0.0, 0.0] },
            ImagePassVertex { pos: [1.0, 1.0] },
            ImagePassVertex { pos: [0.0, 1.0] },
        ];
        let vertex_buffer = eframe::wgpu::util::DeviceExt::create_buffer_init(
            device,
            &eframe::wgpu::util::BufferInitDescriptor {
                label: Some("reference_canvas_image_vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: eframe::wgpu::BufferUsages::VERTEX,
            },
        );
        Self {
            pipeline,
            vertex_buffer,
            texture_bind_group_layout,
            sampler,
        }
    }
}

impl ImagePassCallback {
    fn new(
        target_format: eframe::wgpu::TextureFormat,
        instances: Vec<ImagePassInstance>,
        batches: Vec<ImagePassBatch>,
    ) -> Self {
        Self {
            target_format,
            instances,
            batches,
            prepared: Mutex::new(None),
        }
    }
}

impl eframe::egui_wgpu::CallbackTrait for ImagePassCallback {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        _queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut eframe::wgpu::CommandEncoder,
        callback_resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        if callback_resources.get::<ImagePassResources>().is_none() {
            callback_resources.insert(ImagePassResources::new(device, self.target_format));
        }
        let Some(resources) = callback_resources.get::<ImagePassResources>() else {
            return Vec::new();
        };
        let instance_buffer = eframe::wgpu::util::DeviceExt::create_buffer_init(
            device,
            &eframe::wgpu::util::BufferInitDescriptor {
                label: Some("reference_canvas_image_instances"),
                contents: bytemuck::cast_slice(&self.instances),
                usage: eframe::wgpu::BufferUsages::VERTEX,
            },
        );
        let bind_groups = self
            .batches
            .iter()
            .filter_map(|batch| {
                batch.texture.create_bind_group(
                    device,
                    &resources.texture_bind_group_layout,
                    &resources.sampler,
                )
            })
            .collect::<Vec<_>>();
        *self.prepared.lock().expect("image pass mutex poisoned") = Some(PreparedImagePass {
            instance_buffer,
            bind_groups,
        });
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        callback_resources: &eframe::egui_wgpu::CallbackResources,
    ) {
        let Some(resources) = callback_resources.get::<ImagePassResources>() else {
            return;
        };
        let Some(prepared) = self
            .prepared
            .lock()
            .expect("image pass mutex poisoned")
            .take()
        else {
            return;
        };
        if prepared.bind_groups.len() != self.batches.len() {
            return;
        }
        let clip = info.clip_rect_in_pixels();
        render_pass.set_scissor_rect(
            clip.left_px.max(0) as u32,
            clip.top_px.max(0) as u32,
            clip.width_px.max(0) as u32,
            clip.height_px.max(0) as u32,
        );
        render_pass.set_pipeline(&resources.pipeline);
        render_pass.set_vertex_buffer(0, resources.vertex_buffer.slice(..));
        render_pass.set_vertex_buffer(1, prepared.instance_buffer.slice(..));
        for (index, batch) in self.batches.iter().enumerate() {
            render_pass.set_bind_group(0, &prepared.bind_groups[index], &[]);
            render_pass.draw(0..6, batch.instance_range.clone());
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    input_rect: egui::Rect,
    state: &mut CanvasState,
    render_state: Option<&eframe::egui_wgpu::RenderState>,
    allow_tier_promotion: bool,
    suppress_input: bool,
    debug_labels: bool,
) -> CanvasFrameMetrics {
    let toolbar_rect = editing_textbox_toolbar_rect(ui, state, rect)
        .or_else(|| annotation_style_toolbar_rect(ui, state, rect));
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    if suppress_input {
        state.interaction.drag_state = None;
        state.interaction.marquee_state = None;
        state.interaction.crop_state = None;
        state.interaction.annotation_draft_state = None;
    } else {
        handle_input(ui, rect, input_rect, toolbar_rect, state, &response);
    }

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::theme::BACKGROUND);
    paint_background(&painter, rect, state);
    paint_import_placeholders(&painter, rect, state);
    let image_pass = paint_canvas_items(
        &painter,
        ui,
        rect,
        state,
        render_state,
        allow_tier_promotion,
        debug_labels,
    );
    paint_selection_bounds(&painter, rect, state);
    paint_crop_marquee(&painter, rect, state);
    paint_annotation_draft(&painter, rect, state);
    paint_marquee(&painter, state);
    show_annotation_style_toolbar(ui, rect, state);
    show_textbox_editor(ui, rect, state);
    CanvasFrameMetrics { image_pass }
}

pub fn paint_perf_report() -> PaintPerfReport {
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(960.0, 640.0));
    let pan = egui::Vec2::ZERO;
    let zoom = 1.0;
    let records = (0..400)
        .map(|index| {
            let x = if index < 24 {
                (index % 6) as f32 * 140.0 - 320.0
            } else {
                4000.0 + index as f32 * 48.0
            };
            let y = if index < 24 {
                (index / 6) as f32 * 140.0 - 220.0
            } else {
                2600.0 + index as f32 * 24.0
            };
            crate::data::BoardItemWithAsset {
                item: crate::data::BoardItem {
                    id: index as i64 + 1,
                    asset_id: index as i64 + 1,
                    x,
                    y,
                    scale: 1.0,
                    rotation: 0.0,
                    z_index: index as i32,
                    archived: false,
                },
                asset: crate::data::ImageAsset {
                    id: index as i64 + 1,
                    original_path: std::path::PathBuf::new(),
                    filename: format!("perf-{index}.png"),
                    mime_type: "image/png".to_owned(),
                    width: 512,
                    height: 512,
                    file_size: 0,
                    imported_at: 0,
                },
            }
        })
        .collect::<Vec<_>>();

    let legacy_started_at = std::time::Instant::now();
    let legacy_stats = render::image_paint_stats_for_records(&records, rect, pan, zoom, false);
    let legacy_visibility_pass_ms = legacy_started_at.elapsed().as_secs_f64() * 1000.0;
    let current_started_at = std::time::Instant::now();
    let current_stats = render::image_paint_stats_for_records(&records, rect, pan, zoom, true);
    let current_visibility_pass_ms = current_started_at.elapsed().as_secs_f64() * 1000.0;
    let current_texture_groups =
        render::image_texture_group_count_for_records(&records, rect, pan, zoom);

    PaintPerfReport {
        legacy_visibility_pass_ms,
        current_visibility_pass_ms,
        legacy_considered: legacy_stats.considered,
        legacy_drawn: legacy_stats.drawn,
        current_considered: current_stats.considered,
        current_drawn: current_stats.drawn,
        current_texture_groups,
    }
}

#[cfg(test)]
mod tests {
    use super::geometry::{
        crop_center_offset_local, lattice_phase_screen, lattice_phase_world,
        merge_cropped_item_fields, min_image_scale, pixel_crop_rect_from_local,
        screen_rect_to_local_crop_rect,
    };
    use super::interaction::{
        handle_textbox_primary_press, hit_test_image_resize_handle, hit_test_textbox_resize_handle,
        image_screen_rect, moved_textbox_rect, normalized_textbox_rect,
        open_context_menu_at_pointer, pending_annotation_from_draft, resized_image_scale,
        resized_textbox_rect, textbox_screen_rect,
    };
    use super::render::{
        annotation_paint_stats_for_annotations, background_lod_layers, background_screen_step,
        background_world_step, editing_textbox_toolbar_rect, layout_textbox_galley,
        settled_background_lod_level, textbox_content_rect, textbox_empty_hotspot_rect,
        textbox_font_size, textbox_layout_cache_key, textbox_pointer_region,
        textbox_required_world_height, textbox_text_interactive_rect_from_galley,
        unified_canvas_paint_order_for_state,
    };
    use super::{
        AnnotationDraftState, CanvasImage, CanvasState, CanvasTexture,
        FOCUS_SELECTION_PADDING_SCREEN, ImageResizeHandle, MIN_CANVAS_ZOOM, SelectedLayoutItem,
        TEXTBOX_EDITOR_TOOLBAR_CLEARANCE, TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN,
        TextboxPointerRegion, TextboxResizeHandle, best_compact_layout, horizontal_layout,
        legacy_best_compact_layout, vertical_layout,
    };
    use crate::data::{
        BoardItem, BoardItemWithAsset, CanvasAnnotation, CanvasAnnotationColor,
        CanvasAnnotationKind, CanvasImageCacheTier, CanvasStackItem, CanvasStrokeWidth,
        CanvasTextSize, ImageAsset,
    };
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    fn push_test_image(
        ui: &eframe::egui::Ui,
        state: &mut CanvasState,
        item_id: i64,
        width: u32,
        height: u32,
        scale: f32,
        center: eframe::egui::Pos2,
    ) {
        let texture = ui.ctx().load_texture(
            format!("test-image-{item_id}"),
            eframe::egui::ColorImage::filled([4, 4], eframe::egui::Color32::WHITE),
            eframe::egui::TextureOptions::LINEAR,
        );
        state.document.images.push(CanvasImage {
            record: BoardItemWithAsset {
                item: BoardItem {
                    id: item_id,
                    asset_id: item_id,
                    x: center.x,
                    y: center.y,
                    scale,
                    rotation: 0.0,
                    z_index: 0,
                    archived: false,
                },
                asset: ImageAsset {
                    id: item_id,
                    original_path: PathBuf::from(format!("image-{item_id}.png")),
                    filename: format!("image-{item_id}.png"),
                    mime_type: "image/png".to_owned(),
                    width,
                    height,
                    file_size: 0,
                    imported_at: 0,
                },
            },
            texture: CanvasTexture::Managed(texture),
            loaded_tier: CanvasImageCacheTier::Working,
            animation: None,
        });
    }

    fn default_text_size() -> CanvasTextSize {
        CanvasTextSize::default()
    }

    fn default_annotation_color() -> CanvasAnnotationColor {
        CanvasAnnotationColor::Default
    }

    fn default_stroke_width() -> CanvasStrokeWidth {
        CanvasStrokeWidth::default()
    }

    fn test_record(item_id: i64, asset_id: i64, width: u32, height: u32) -> BoardItemWithAsset {
        BoardItemWithAsset {
            item: BoardItem {
                id: item_id,
                asset_id,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                z_index: item_id as i32,
                archived: false,
            },
            asset: ImageAsset {
                id: asset_id,
                original_path: PathBuf::from(format!("image-{asset_id}.png")),
                filename: format!("image-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height,
                file_size: 0,
                imported_at: 0,
            },
        }
    }

    fn native_loaded_item(
        record: BoardItemWithAsset,
        texture: Arc<crate::jobs::NativeTextureHandle>,
    ) -> crate::jobs::LoadedCanvasItem {
        crate::jobs::LoadedCanvasItem {
            target_board_id: 1,
            queue_id: None,
            preview_rgba: vec![255; record.asset.width as usize * record.asset.height as usize * 4],
            preview_width: record.asset.width,
            preview_height: record.asset.height,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Native(texture),
            loaded_tier: CanvasImageCacheTier::Working,
            existing_load_diagnostics: None,
            record,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        }
    }

    fn animated_loaded_item(
        _ui: &eframe::egui::Ui,
        record: BoardItemWithAsset,
    ) -> crate::jobs::LoadedCanvasItem {
        let first_frame = vec![0, 0, 255, 255];
        let second_frame = vec![0, 255, 0, 255];
        crate::jobs::LoadedCanvasItem {
            target_board_id: 1,
            queue_id: None,
            preview_rgba: first_frame.clone(),
            preview_width: 1,
            preview_height: 1,
            animated_preview: Some(crate::jobs::AnimatedPreview {
                frames: vec![
                    crate::jobs::AnimatedPreviewFrame {
                        rgba: first_frame,
                        delay_seconds: 0.1,
                    },
                    crate::jobs::AnimatedPreviewFrame {
                        rgba: second_frame,
                        delay_seconds: 0.1,
                    },
                ],
                width: 1,
                height: 1,
            }),
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_tier: CanvasImageCacheTier::Working,
            existing_load_diagnostics: None,
            record,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        }
    }

    #[test]
    fn deleting_last_shared_native_texture_triggers_single_cleanup() {
        let ctx = eframe::egui::Context::default();
        let mut state = CanvasState::default();
        let cleanup_count = Arc::new(AtomicUsize::new(0));
        let handle = crate::jobs::NativeTextureHandle::for_tests(
            eframe::egui::TextureId::User(101),
            [4, 4],
            {
                let cleanup_count = cleanup_count.clone();
                move |_| {
                    cleanup_count.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        let first = test_record(1, 1, 4, 4);

        state.add_loaded_item(&ctx, native_loaded_item(first, handle.clone()));
        state.add_reference_item(test_record(2, 1, 4, 4)).unwrap();
        drop(handle);

        state.delete_items(&[1]);
        assert_eq!(cleanup_count.load(Ordering::SeqCst), 0);

        state.delete_items(&[2]);
        assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn crop_replacement_keeps_old_native_texture_until_last_shared_reference_drops() {
        let ctx = eframe::egui::Context::default();
        let mut state = CanvasState::default();
        let old_cleanup = Arc::new(AtomicUsize::new(0));
        let new_cleanup = Arc::new(AtomicUsize::new(0));
        let old_texture = crate::jobs::NativeTextureHandle::for_tests(
            eframe::egui::TextureId::User(201),
            [4, 4],
            {
                let old_cleanup = old_cleanup.clone();
                move |_| {
                    old_cleanup.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        let new_texture = crate::jobs::NativeTextureHandle::for_tests(
            eframe::egui::TextureId::User(202),
            [2, 2],
            {
                let new_cleanup = new_cleanup.clone();
                move |_| {
                    new_cleanup.fetch_add(1, Ordering::SeqCst);
                }
            },
        );

        state.add_loaded_item(
            &ctx,
            native_loaded_item(test_record(1, 1, 4, 4), old_texture.clone()),
        );
        state.add_reference_item(test_record(2, 1, 4, 4)).unwrap();
        drop(old_texture);

        state
            .apply_cropped_item(
                &ctx,
                crate::jobs::CroppedCanvasItem {
                    item_id: 1,
                    stored_path: PathBuf::from("crop.png"),
                    width: 2,
                    height: 2,
                    file_size: 0,
                    canvas_rgba: vec![200; 16],
                    canvas_width: 2,
                    canvas_height: 2,
                    preview_rgba: vec![200; 16],
                    preview_width: 2,
                    preview_height: 2,
                    preview_texture: crate::jobs::ImportedPreviewTexture::Native(
                        new_texture.clone(),
                    ),
                    loaded_tier: CanvasImageCacheTier::Working,
                    crop_offset_local: eframe::egui::Vec2::ZERO,
                },
                ImageAsset {
                    id: 2,
                    original_path: PathBuf::from("crop.png"),
                    filename: "crop.png".to_owned(),
                    mime_type: "image/png".to_owned(),
                    width: 2,
                    height: 2,
                    file_size: 0,
                    imported_at: 0,
                },
                BoardItem {
                    id: 1,
                    asset_id: 2,
                    x: 0.0,
                    y: 0.0,
                    scale: 1.0,
                    rotation: 0.0,
                    z_index: 1,
                    archived: false,
                },
            )
            .unwrap();
        drop(new_texture);

        assert_eq!(old_cleanup.load(Ordering::SeqCst), 0);
        assert_eq!(new_cleanup.load(Ordering::SeqCst), 0);

        state.delete_items(&[2]);
        assert_eq!(old_cleanup.load(Ordering::SeqCst), 1);
        assert_eq!(new_cleanup.load(Ordering::SeqCst), 0);

        state.delete_items(&[1]);
        assert_eq!(new_cleanup.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reference_items_preserve_gif_animation_after_source_item_is_removed() {
        let mut state = CanvasState::default();

        eframe::egui::__run_test_ui(|ui| {
            state.add_loaded_item(ui.ctx(), animated_loaded_item(ui, test_record(1, 1, 1, 1)));
        });
        state.add_reference_item(test_record(2, 1, 1, 1)).unwrap();

        assert!(state.document.images[1].animation.is_some());
        assert_eq!(
            state.advance_animated_image(2, 0.15).unwrap().0,
            vec![0, 255, 0, 255]
        );

        state.delete_items(&[1]);

        assert_eq!(
            state.advance_animated_image(2, 0.25).unwrap().0,
            vec![0, 0, 255, 255]
        );
        assert!(
            state
                .document
                .images
                .iter()
                .any(|image| image.record.item.id == 2 && image.animation.is_some())
        );
    }

    #[test]
    fn converts_drag_rect_to_crop_pixels_and_live_scale_position() {
        let screen_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(40.0, 20.0),
            eframe::egui::pos2(240.0, 180.0),
        );
        let local_rect = screen_rect_to_local_crop_rect(screen_rect, screen_rect, 100, 80).unwrap();
        let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 80).unwrap();

        assert_eq!(crop_rect.x, 0);
        assert_eq!(crop_rect.y, 0);
        assert_eq!(crop_rect.width, 100);
        assert_eq!(crop_rect.height, 80);

        let item = BoardItem {
            id: 1,
            asset_id: 1,
            x: 200.0,
            y: 120.0,
            scale: 2.0,
            rotation: 0.0,
            z_index: 0,
            archived: false,
        };
        let asset = ImageAsset {
            id: 1,
            original_path: PathBuf::from("image.png"),
            filename: "image.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 100,
            height: 80,
            file_size: 0,
            imported_at: 0,
        };
        let kept_local = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::pos2(40.0, 50.0),
        );
        let crop_offset_local = crop_center_offset_local(&asset, kept_local);
        let mut merged_item = item.clone();
        merge_cropped_item_fields(&mut merged_item, 2, crop_offset_local);

        assert_eq!(merged_item.asset_id, 2);
        assert_eq!(merged_item.x, 150.0);
        assert_eq!(merged_item.y, 110.0);
    }

    #[test]
    fn rounds_local_rect_outward_to_cover_selected_pixels() {
        let local_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(10.2, 11.8),
            eframe::egui::pos2(40.1, 30.2),
        );
        let crop_rect = pixel_crop_rect_from_local(local_rect, 100, 100).unwrap();

        assert_eq!(crop_rect.x, 10);
        assert_eq!(crop_rect.y, 11);
        assert_eq!(crop_rect.width, 31);
        assert_eq!(crop_rect.height, 20);
    }

    #[test]
    fn textbox_rect_enforces_minimum_size() {
        let rect = normalized_textbox_rect(
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::pos2(32.0, 56.0),
        );

        assert_eq!(rect.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(rect.width(), 240.0);
        assert_eq!(rect.height(), 56.0);
    }

    #[test]
    fn textbox_click_draft_uses_default_note_size() {
        let pending = pending_annotation_from_draft(AnnotationDraftState {
            kind: CanvasAnnotationKind::Textbox,
            start_world: eframe::egui::pos2(20.0, 40.0),
            current_world: eframe::egui::pos2(20.0, 40.0),
        })
        .unwrap();

        assert_eq!(pending.start_world, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(pending.end_world, eframe::egui::pos2(260.0, 96.0));
    }

    #[test]
    fn textbox_drag_draft_preserves_explicit_size() {
        let pending = pending_annotation_from_draft(AnnotationDraftState {
            kind: CanvasAnnotationKind::Textbox,
            start_world: eframe::egui::pos2(20.0, 40.0),
            current_world: eframe::egui::pos2(340.0, 120.0),
        })
        .unwrap();

        assert_eq!(pending.start_world, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(pending.end_world, eframe::egui::pos2(340.0, 120.0));
    }

    #[test]
    fn focus_selection_in_view_noops_without_selection() {
        let mut state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(960.0, 640.0),
        );
        let target_rect = canvas_rect.shrink2(eframe::egui::vec2(0.0, 120.0));

        assert!(!state.focus_selection_in_view(canvas_rect, target_rect));
    }

    #[test]
    fn focus_selection_in_view_fits_selected_bounds_inside_target_rect() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(960.0, 640.0),
            );
            let target_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(960.0, 420.0),
            );
            push_test_image(
                ui,
                &mut state,
                1,
                300,
                180,
                1.0,
                eframe::egui::pos2(-300.0, -120.0),
            );
            push_test_image(
                ui,
                &mut state,
                2,
                180,
                320,
                1.0,
                eframe::egui::pos2(260.0, 180.0),
            );
            state.select_items(&[1, 2]);

            assert!(state.focus_selection_in_view(canvas_rect, target_rect));

            let selection_rect = state.selection_screen_rect(canvas_rect).unwrap();
            assert!(
                target_rect
                    .shrink(FOCUS_SELECTION_PADDING_SCREEN)
                    .contains_rect(selection_rect)
            );
        });
    }

    #[test]
    fn import_placeholders_replace_by_queue_id_and_remove_cleanly() {
        let mut state = CanvasState::default();
        state.add_import_placeholder(
            7,
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::vec2(100.0, 80.0),
            "first".to_owned(),
        );
        state.add_import_placeholder(
            7,
            eframe::egui::pos2(30.0, 40.0),
            eframe::egui::vec2(120.0, 90.0),
            "updated".to_owned(),
        );

        assert_eq!(state.import_placeholder_count(), 1);
        state.remove_import_placeholder(7);
        assert_eq!(state.import_placeholder_count(), 0);
    }

    #[test]
    fn hover_preview_placeholders_replace_and_clear_without_touching_queued_placeholders() {
        let mut state = CanvasState::default();
        state.add_import_placeholder(
            1,
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(100.0, 80.0),
            "queued".to_owned(),
        );

        state.set_hover_preview_placeholders(vec![
            (
                eframe::egui::pos2(10.0, 10.0),
                eframe::egui::vec2(50.0, 50.0),
            ),
            (
                eframe::egui::pos2(70.0, 10.0),
                eframe::egui::vec2(50.0, 50.0),
            ),
        ]);
        assert_eq!(state.hover_preview_placeholder_count(), 2);
        assert_eq!(state.import_placeholder_count(), 1);

        state.set_hover_preview_placeholders(vec![(
            eframe::egui::pos2(20.0, 20.0),
            eframe::egui::vec2(40.0, 40.0),
        )]);
        assert_eq!(state.hover_preview_placeholder_count(), 1);

        state.clear_hover_preview_placeholders();
        assert_eq!(state.hover_preview_placeholder_count(), 0);
        assert_eq!(state.import_placeholder_count(), 1);
    }

    #[test]
    fn textbox_multiline_content_requires_more_than_min_height() {
        assert!(textbox_required_world_height(80.0, 1.0) > 56.0);

        eframe::egui::__run_test_ui(|ui| {
            let mut text =
                "First line\nSecond line\nThird line\nFourth line\nFifth line".to_owned();
            let output = eframe::egui::TextEdit::multiline(&mut text)
                .desired_width(80.0)
                .font(eframe::egui::FontId::proportional(24.0))
                .show(ui);

            assert!(output.galley.rows.len() >= 5);
        });
    }

    #[test]
    fn larger_text_sizes_require_more_textbox_height() {
        assert!(textbox_font_size(CanvasTextSize::Xl) > textbox_font_size(CanvasTextSize::S));
        assert!(textbox_font_size(CanvasTextSize::M) > textbox_font_size(CanvasTextSize::S));
        assert!(textbox_font_size(CanvasTextSize::L) > textbox_font_size(CanvasTextSize::M));
        assert!(textbox_font_size(CanvasTextSize::Xl) > textbox_font_size(CanvasTextSize::L));
    }

    #[test]
    fn textbox_content_rect_respects_available_bounds() {
        let textbox_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::vec2(8.0, 6.0),
        );
        let content_rect = textbox_content_rect(textbox_rect);

        assert!(textbox_rect.contains(content_rect.min));
        assert!(textbox_rect.contains(content_rect.max));
        assert!(content_rect.width() <= textbox_rect.width());
        assert!(content_rect.height() <= textbox_rect.height());
    }

    #[test]
    fn textbox_content_rect_stays_inside_zoomed_out_textbox() {
        let mut state = CanvasState::default();
        state.zoom = MIN_CANVAS_ZOOM;
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotation = CanvasAnnotation {
            id: 11,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: "zoomed out".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        };

        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let content_rect = textbox_content_rect(textbox_rect);

        assert!(textbox_rect.contains(content_rect.min));
        assert!(textbox_rect.contains(content_rect.max));
        assert!(content_rect.width() <= textbox_rect.width());
        assert!(content_rect.height() <= textbox_rect.height());
    }

    #[test]
    fn textbox_toolbar_aligns_to_content_start_when_space_allows() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(960.0, 640.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 71,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 80.0,
                start_y: 200.0,
                end_x: 420.0,
                end_y: 320.0,
                text: "toolbar".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(71);

            let annotation = state.annotation_by_id(71).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

            assert!((toolbar_rect.left() - textbox_rect.left()).abs() < 0.1);
        });
    }

    #[test]
    fn textbox_toolbar_preserves_gap_above_when_space_allows() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(960.0, 640.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 74,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 80.0,
                start_y: 200.0,
                end_x: 420.0,
                end_y: 320.0,
                text: "toolbar".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(74);

            let annotation = state.annotation_by_id(74).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

            assert!(
                (textbox_rect.top() - toolbar_rect.bottom() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE)
                    .abs()
                    < 0.1
            );
        });
    }

    #[test]
    fn textbox_toolbar_clamps_to_top_margin_when_above_gap_does_not_fit() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(960.0, 640.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 72,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 80.0,
                start_y: -300.0,
                end_x: 420.0,
                end_y: -180.0,
                text: "toolbar".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(72);

            let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

            assert!(
                (toolbar_rect.top() - (canvas_rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN))
                    .abs()
                    < 0.1
            );
        });
    }

    #[test]
    fn textbox_toolbar_clamps_right_without_unnecessary_shift() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(420.0, 320.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 73,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 220.0,
                start_y: 140.0,
                end_x: 460.0,
                end_y: 240.0,
                text: "toolbar".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(73);

            let toolbar_rect = editing_textbox_toolbar_rect(ui, &state, canvas_rect).unwrap();

            assert!(
                toolbar_rect.right()
                    <= canvas_rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN + 0.1
            );
            assert!(
                toolbar_rect.left()
                    >= canvas_rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - 0.1
            );
        });
    }

    #[test]
    fn textbox_layout_cache_reuses_entry_for_identical_inputs() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(
                ui,
                1,
                "cache me",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            );
            let second = state.cached_textbox_layout(
                ui,
                1,
                "cache me",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            );

            assert!(Arc::ptr_eq(&first.galley, &second.galley));
            assert_eq!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_layout_cache_replaces_entry_when_text_changes() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(
                ui,
                1,
                "before",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            );
            let second = state.cached_textbox_layout(
                ui,
                1,
                "after",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            );

            assert!(!Arc::ptr_eq(&first.galley, &second.galley));
            assert_ne!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_layout_cache_replaces_entry_when_zoom_changes() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let first = state.cached_textbox_layout(
                ui,
                1,
                "zoom",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
            );
            let second = state.cached_textbox_layout(
                ui,
                1,
                "zoom",
                default_text_size(),
                default_annotation_color(),
                120.0,
                2.0,
            );

            assert!(!Arc::ptr_eq(&first.galley, &second.galley));
            assert_ne!(first.key, second.key);
        });
    }

    #[test]
    fn textbox_editor_writeback_replaces_cached_layout_entry() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let textbox_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(240.0, 56.0),
            );
            let text_rect = textbox_content_rect(textbox_rect);
            let mut text = "updated text".to_owned();
            let mut layouter =
                |ui: &eframe::egui::Ui, text: &dyn eframe::egui::TextBuffer, width: f32| {
                    layout_textbox_galley(
                        ui,
                        text.as_str(),
                        default_text_size(),
                        default_annotation_color(),
                        width,
                        1.0,
                    )
                };

            state.cache_textbox_layout(
                9,
                "stale",
                default_text_size(),
                default_annotation_color(),
                text_rect.width(),
                1.0,
                layout_textbox_galley(
                    ui,
                    "stale",
                    default_text_size(),
                    default_annotation_color(),
                    text_rect.width(),
                    1.0,
                ),
            );

            let output = ui
                .allocate_ui(text_rect.size(), |ui| {
                    eframe::egui::TextEdit::multiline(&mut text)
                        .desired_width(text_rect.width())
                        .layouter(&mut layouter)
                        .show(ui)
                })
                .inner;
            let entry = state.cache_textbox_layout(
                9,
                &text,
                default_text_size(),
                default_annotation_color(),
                text_rect.width(),
                1.0,
                output.galley,
            );

            assert_eq!(
                entry.key,
                textbox_layout_cache_key(
                    &text,
                    default_text_size(),
                    default_annotation_color(),
                    text_rect.width(),
                    1.0,
                )
            );
            assert_eq!(
                state.render.textbox_layout_cache.get(&9).unwrap().key,
                textbox_layout_cache_key(
                    &text,
                    default_text_size(),
                    default_annotation_color(),
                    text_rect.width(),
                    1.0,
                )
            );
        });
    }

    #[test]
    fn textbox_move_applies_pointer_delta() {
        let moved = moved_textbox_rect(
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(100.0, 100.0),
            eframe::egui::pos2(130.0, 145.0),
        );

        assert_eq!(moved.min, eframe::egui::pos2(50.0, 85.0));
        assert_eq!(moved.max, eframe::egui::pos2(290.0, 141.0));
    }

    #[test]
    fn textbox_resize_respects_minimum_size() {
        let resized = resized_textbox_rect(
            TextboxResizeHandle::BottomRight,
            eframe::egui::pos2(20.0, 40.0),
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(30.0, 50.0),
        );

        assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(resized.width(), 240.0);
        assert_eq!(resized.height(), 56.0);
    }

    #[test]
    fn textbox_edge_resize_moves_one_side_only() {
        let resized = resized_textbox_rect(
            TextboxResizeHandle::Right,
            eframe::egui::pos2(20.0, 68.0),
            eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(20.0, 40.0),
                eframe::egui::pos2(260.0, 96.0),
            ),
            eframe::egui::pos2(320.0, 20.0),
        );

        assert_eq!(resized.min, eframe::egui::pos2(20.0, 40.0));
        assert_eq!(resized.max, eframe::egui::pos2(320.0, 96.0));
    }

    #[test]
    fn textbox_resize_hit_test_matches_visible_right_edge_zone() {
        let state = CanvasState::default();
        let canvas_rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotation = CanvasAnnotation {
            id: 1,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: String::new(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        };

        let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
        let pointer_pos = eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

        assert_eq!(
            hit_test_textbox_resize_handle(&state, canvas_rect, &annotation, pointer_pos),
            Some(TextboxResizeHandle::Right)
        );
    }

    #[test]
    fn image_resize_hit_test_matches_corner_zone() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            push_test_image(
                ui,
                &mut state,
                21,
                200,
                100,
                1.0,
                eframe::egui::pos2(200.0, 160.0),
            );
            let image = state.image_by_item_id(21).unwrap();
            let image_rect = image_screen_rect(&state, canvas_rect, image);
            let pointer_pos = image_rect.right_bottom() + eframe::egui::vec2(8.0, 8.0);

            assert_eq!(
                hit_test_image_resize_handle(&state, canvas_rect, image, pointer_pos),
                Some(ImageResizeHandle::BottomRight)
            );
        });
    }

    #[test]
    fn image_resize_scales_proportionally_from_center() {
        let asset = ImageAsset {
            id: 22,
            original_path: PathBuf::from("image-22.png"),
            filename: "image-22.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 200,
            height: 100,
            file_size: 0,
            imported_at: 0,
        };
        let center = eframe::egui::pos2(300.0, 240.0);
        let start_radius = 111.8034;
        let pointer_pos = center + eframe::egui::vec2(200.0, 100.0);

        let scale = resized_image_scale(&asset, 1.0, start_radius, center, pointer_pos);

        assert!((scale - 2.0).abs() < 0.01);
    }

    #[test]
    fn image_resize_respects_minimum_scale() {
        let asset = ImageAsset {
            id: 23,
            original_path: PathBuf::from("image-23.png"),
            filename: "image-23.png".to_owned(),
            mime_type: "image/png".to_owned(),
            width: 400,
            height: 200,
            file_size: 0,
            imported_at: 0,
        };

        let scale = resized_image_scale(
            &asset,
            1.0,
            223.6068,
            eframe::egui::pos2(300.0, 240.0),
            eframe::egui::pos2(300.5, 240.5),
        );

        assert_eq!(scale, min_image_scale(&asset));
    }

    #[test]
    fn apply_target_area_to_selected_images_normalizes_displayed_area() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            push_test_image(
                ui,
                &mut state,
                31,
                200,
                100,
                1.0,
                eframe::egui::pos2(160.0, 120.0),
            );
            push_test_image(
                ui,
                &mut state,
                32,
                100,
                100,
                1.0,
                eframe::egui::pos2(360.0, 120.0),
            );
            state.select_items(&[31, 32]);

            let changed = state.apply_target_area_to_selected_images(90_000.0);

            assert_eq!(changed, 2);
            for item_id in [31, 32] {
                let image = state.image_by_item_id(item_id).unwrap();
                let area = image.record.asset.width as f32
                    * image.record.asset.height as f32
                    * image.record.item.scale
                    * image.record.item.scale;
                assert!((area - 90_000.0).abs() < 0.1);
            }
        });
    }

    #[test]
    fn average_selected_image_area_uses_displayed_area() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            push_test_image(
                ui,
                &mut state,
                41,
                100,
                100,
                1.0,
                eframe::egui::pos2(120.0, 120.0),
            );
            push_test_image(
                ui,
                &mut state,
                42,
                200,
                100,
                1.0,
                eframe::egui::pos2(320.0, 120.0),
            );
            state.select_items(&[41, 42]);

            let average_area = state.average_selected_image_area().unwrap();

            assert!((average_area - 15_000.0).abs() < 0.1);
        });
    }

    #[test]
    fn textbox_text_interactive_rect_matches_galley_bounds_for_non_empty_text() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            let annotation = CanvasAnnotation {
                id: 14,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 160.0,
                text: "short".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            };

            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            let layout = state.cached_textbox_layout(
                ui,
                annotation.id,
                &annotation.text,
                default_text_size(),
                default_annotation_color(),
                content_rect.width(),
                1.0,
            );
            let text_rect = textbox_text_interactive_rect_from_galley(
                ui,
                content_rect,
                &layout.galley,
                default_text_size(),
                1.0,
                false,
            );

            assert!(text_rect.width() < content_rect.width());
            assert!(text_rect.height() <= content_rect.height());
            assert!(text_rect.contains(content_rect.min));
        });
    }

    #[test]
    fn textbox_empty_hotspot_preserves_draggable_body_area() {
        eframe::egui::__run_test_ui(|ui| {
            let content_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(24.0, 24.0),
                eframe::egui::vec2(280.0, 96.0),
            );
            let hotspot = textbox_empty_hotspot_rect(ui, content_rect, default_text_size(), 1.0);

            assert!(hotspot.width() < content_rect.width());
            assert!(hotspot.height() < content_rect.height());
            assert!(hotspot.contains(content_rect.min));
            assert!(!hotspot.contains(eframe::egui::pos2(
                content_rect.right() - 8.0,
                content_rect.bottom() - 8.0,
            )));
        });
    }

    #[test]
    fn textbox_body_press_on_unselected_textbox_starts_pending_move() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 15,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "short".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });

            let annotation = state.annotation_by_id(15).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos =
                eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.document.selected_annotation_ids, vec![15]);
            assert!(matches!(
                state.interaction.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { ref annotation_ids, .. })
                    if annotation_ids == &vec![15]
            ));
        });
    }

    #[test]
    fn textbox_edge_press_on_unselected_textbox_selects_without_resizing() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 18,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "short".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.document.selected_annotation_ids.clear();

            let annotation = state.annotation_by_id(18).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos =
                eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.document.selected_annotation_ids, vec![18]);
            assert!(matches!(
                state.interaction.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { ref annotation_ids, .. })
                    if annotation_ids == &vec![18]
            ));
        });
    }

    #[test]
    fn textbox_edge_press_on_selected_textbox_starts_resize() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 19,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "short".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.document.selected_annotation_ids = vec![19];

            let annotation = state.annotation_by_id(19).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos =
                eframe::egui::pos2(textbox_rect.right() + 8.0, textbox_rect.center().y);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert!(matches!(
                state.interaction.textbox_transform_state,
                Some(super::TextboxTransformState::Resizing {
                    annotation_id: 19,
                    dragged_handle: TextboxResizeHandle::Right,
                    ..
                })
            ));
        });
    }

    #[test]
    fn textbox_body_press_while_editing_empty_exits_edit_and_starts_pending_move() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 16,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: String::new(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(16);

            let annotation = state.annotation_by_id(16).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos =
                eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.interaction.editing_text_annotation_id, None);
            assert!(state.annotation_by_id(16).is_some());
            assert!(matches!(
                state.interaction.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { ref annotation_ids, .. })
                    if annotation_ids == &vec![16]
            ));
        });
    }

    #[test]
    fn textbox_body_press_on_selected_multi_textbox_keeps_group_move() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            for (id, start_x) in [(21, 20.0), (22, 360.0)] {
                state.add_annotation(CanvasAnnotation {
                    id,
                    kind: CanvasAnnotationKind::Textbox,
                    start_x,
                    start_y: 40.0,
                    end_x: start_x + 220.0,
                    end_y: 140.0,
                    text: "group".to_owned(),
                    text_size: default_text_size(),
                    stroke_width: default_stroke_width(),
                    color: default_annotation_color(),
                    background_enabled: false,
                    archived: false,
                });
            }
            state.document.selected_annotation_ids = vec![21, 22];

            let annotation = state.annotation_by_id(21).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let pointer_pos =
                eframe::egui::pos2(textbox_rect.center().x, textbox_rect.bottom() - 32.0);

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.document.selected_annotation_ids, vec![21, 22]);
            assert!(matches!(
                state.interaction.textbox_transform_state,
                Some(super::TextboxTransformState::PendingMove { ref annotation_ids, .. })
                    if annotation_ids == &vec![21, 22]
            ));
        });
    }

    #[test]
    fn textbox_text_press_while_editing_stays_editor_owned() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_max(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::pos2(800.0, 600.0),
            );
            state.add_annotation(CanvasAnnotation {
                id: 17,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "hello world".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.begin_textbox_edit(17);

            let annotation = state.annotation_by_id(17).unwrap().clone();
            let textbox_rect = textbox_screen_rect(&state, canvas_rect, &annotation);
            let content_rect = textbox_content_rect(textbox_rect);
            let layout = state.cached_textbox_layout(
                ui,
                annotation.id,
                &annotation.text,
                default_text_size(),
                default_annotation_color(),
                content_rect.width(),
                1.0,
            );
            let text_rect = textbox_text_interactive_rect_from_galley(
                ui,
                content_rect,
                &layout.galley,
                default_text_size(),
                1.0,
                false,
            );
            let pointer_pos = text_rect.center();

            assert_eq!(
                textbox_pointer_region(ui, canvas_rect, &mut state, &annotation, pointer_pos),
                Some(TextboxPointerRegion::Text)
            );

            assert!(handle_textbox_primary_press(
                ui,
                canvas_rect,
                &mut state,
                &annotation,
                pointer_pos,
            ));
            assert_eq!(state.interaction.editing_text_annotation_id, Some(17));
            assert!(state.interaction.textbox_transform_state.is_none());
        });
    }

    #[test]
    fn begin_textbox_edit_selects_textbox_without_placeholder_content() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 5,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: String::new(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });

        state.begin_textbox_edit(5);

        assert_eq!(state.interaction.editing_text_annotation_id, Some(5));
        assert_eq!(state.document.selected_annotation_ids, vec![5]);
        assert!(
            state
                .annotation_by_id(5)
                .is_some_and(|annotation| annotation.text.is_empty())
        );
    }

    #[test]
    fn finishing_empty_textbox_edit_removes_and_archives_annotation() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 7,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 80.0,
            text: String::new(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });

        state.begin_textbox_edit(7);
        state.finish_textbox_edit();

        assert!(state.document.annotations.is_empty());
        assert_eq!(
            state.take_document_changes().archived_annotation_ids,
            vec![7]
        );
    }

    #[test]
    fn add_annotation_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.cache_textbox_layout(
                5,
                "cached",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                layout_textbox_galley(
                    ui,
                    "cached",
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    1.0,
                ),
            );

            state.add_annotation(CanvasAnnotation {
                id: 5,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: String::new(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });

            assert!(!state.render.textbox_layout_cache.contains_key(&5));
        });
    }

    #[test]
    fn add_annotations_clears_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.cache_textbox_layout(
                1,
                "one",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                layout_textbox_galley(
                    ui,
                    "one",
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    1.0,
                ),
            );
            state.cache_textbox_layout(
                2,
                "two",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                layout_textbox_galley(
                    ui,
                    "two",
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    1.0,
                ),
            );

            state.add_annotations(Vec::new());

            assert!(state.render.textbox_layout_cache.is_empty());
        });
    }

    #[test]
    fn remove_annotation_locally_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.add_annotation(CanvasAnnotation {
                id: 3,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: "hello".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.cache_textbox_layout(
                3,
                "hello",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                layout_textbox_galley(
                    ui,
                    "hello",
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    1.0,
                ),
            );

            state.remove_annotation_locally(3);

            assert!(!state.render.textbox_layout_cache.contains_key(&3));
        });
    }

    #[test]
    fn set_textbox_world_rect_invalidates_textbox_layout_cache() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            state.add_annotation(CanvasAnnotation {
                id: 4,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 260.0,
                end_y: 96.0,
                text: "resize".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
            state.cache_textbox_layout(
                4,
                "resize",
                default_text_size(),
                default_annotation_color(),
                120.0,
                1.0,
                layout_textbox_galley(
                    ui,
                    "resize",
                    default_text_size(),
                    default_annotation_color(),
                    120.0,
                    1.0,
                ),
            );

            assert!(state.set_textbox_world_rect(
                4,
                eframe::egui::Rect::from_min_max(
                    eframe::egui::pos2(20.0, 40.0),
                    eframe::egui::pos2(320.0, 120.0),
                ),
            ));
            assert!(!state.render.textbox_layout_cache.contains_key(&4));
        });
    }

    #[test]
    fn marquee_selection_can_select_textbox_when_no_images_intersect() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 12,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: "textbox".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });

        state.set_marquee_selection(Vec::new(), vec![12], false);

        assert!(state.document.selected_item_ids.is_empty());
        assert_eq!(state.document.selected_annotation_ids, vec![12]);
    }

    #[test]
    fn marquee_selection_can_add_multiple_textboxes() {
        let mut state = CanvasState::default();
        state.add_annotation(CanvasAnnotation {
            id: 12,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 20.0,
            start_y: 40.0,
            end_x: 260.0,
            end_y: 96.0,
            text: "textbox".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });
        state.add_annotation(CanvasAnnotation {
            id: 13,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 280.0,
            start_y: 40.0,
            end_x: 420.0,
            end_y: 96.0,
            text: "textbox".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });

        state.set_marquee_selection(Vec::new(), vec![12], false);
        state.set_marquee_selection(Vec::new(), vec![13], true);

        assert_eq!(state.document.selected_annotation_ids, vec![12, 13]);
    }

    #[test]
    fn annotation_paint_stats_cull_offscreen_textboxes() {
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let annotations = vec![
            CanvasAnnotation {
                id: 30,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 20.0,
                start_y: 40.0,
                end_x: 320.0,
                end_y: 180.0,
                text: "visible".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            },
            CanvasAnnotation {
                id: 31,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 2000.0,
                start_y: 40.0,
                end_x: 2300.0,
                end_y: 180.0,
                text: "offscreen-a".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            },
            CanvasAnnotation {
                id: 32,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 2400.0,
                start_y: 260.0,
                end_x: 2700.0,
                end_y: 400.0,
                text: "offscreen-b".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            },
            CanvasAnnotation {
                id: 33,
                kind: CanvasAnnotationKind::Textbox,
                start_x: 2800.0,
                start_y: -120.0,
                end_x: 3100.0,
                end_y: 20.0,
                text: "offscreen-c".to_owned(),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            },
        ];

        let legacy = annotation_paint_stats_for_annotations(
            &annotations,
            rect,
            eframe::egui::Vec2::ZERO,
            8.0,
            false,
        );
        let current = annotation_paint_stats_for_annotations(
            &annotations,
            rect,
            eframe::egui::Vec2::ZERO,
            8.0,
            true,
        );

        assert_eq!(legacy.considered, 4);
        assert_eq!(legacy.drawn, 4);
        assert_eq!(current.considered, 4);
        assert_eq!(current.drawn, 1);
    }

    #[test]
    fn single_selected_annotation_id_is_none_when_selection_is_empty() {
        let state = CanvasState::default();
        assert_eq!(state.single_selected_annotation_id(), None);
    }

    #[test]
    fn merge_only_updates_crop_owned_item_fields() {
        let mut item = BoardItem {
            id: 7,
            asset_id: 2,
            x: 12.0,
            y: 18.0,
            scale: 1.5,
            rotation: 0.25,
            z_index: 9,
            archived: false,
        };

        merge_cropped_item_fields(&mut item, 11, eframe::egui::vec2(80.0, 96.0));

        assert_eq!(item.asset_id, 11);
        assert_eq!(item.x, 132.0);
        assert_eq!(item.y, 162.0);
        assert_eq!(item.scale, 1.5);
        assert_eq!(item.rotation, 0.25);
        assert_eq!(item.z_index, 9);
        assert!(!item.archived);
    }

    #[test]
    fn pending_crop_guard_blocks_second_crop_until_cleared() {
        let mut state = super::CanvasState::default();
        let handle = crate::jobs::NativeTextureHandle::for_tests(
            eframe::egui::TextureId::User(42),
            [4, 4],
            |_| {},
        );
        state.document.images.push(CanvasImage {
            record: test_record(42, 42, 4, 4),
            texture: CanvasTexture::Native(handle),
            loaded_tier: CanvasImageCacheTier::Working,
            animation: None,
        });

        assert!(state.can_start_crop_on_item(42));
        state.mark_crop_pending(42);
        assert!(!state.can_start_crop_on_item(42));
        state.clear_crop_pending(42);
        assert!(state.can_start_crop_on_item(42));
    }

    #[test]
    fn crop_guard_rejects_svg_items() {
        let mut state = super::CanvasState::default();
        let mut record = test_record(42, 42, 4, 4);
        record.asset.mime_type = "image/svg+xml".to_owned();
        state.document.images.push(CanvasImage {
            record,
            texture: CanvasTexture::Svg {
                uri: "memory://test.svg".to_owned(),
                bytes: Arc::<[u8]>::from(b"<svg viewBox='0 0 4 4'/>".as_slice()),
                size: [4, 4],
            },
            loaded_tier: CanvasImageCacheTier::Working,
            animation: None,
        });

        assert!(!state.can_start_crop_on_item(42));
    }

    #[test]
    fn lattice_phase_is_preserved_across_zoom_change_with_same_lod() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let world_point = eframe::egui::pos2(150.0, 94.0);
        let world_step = background_world_step(0);

        state.zoom = 0.95;
        let screen_origin_a = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point_a = state.world_to_screen(world_point, rect);
        let phase_a = lattice_phase_screen(
            screen_origin_a.x,
            screen_point_a.x,
            background_screen_step(0, state.zoom),
        );

        state.zoom = 1.05;
        let screen_origin_b = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point_b = state.world_to_screen(world_point, rect);
        let phase_b = lattice_phase_screen(
            screen_origin_b.x,
            screen_point_b.x,
            background_screen_step(0, state.zoom),
        );

        let world_phase = lattice_phase_world(world_point.x, world_step);
        assert!((phase_a - world_phase).abs() < 0.0001);
        assert!((phase_b - world_phase).abs() < 0.0001);
        assert!((phase_a - phase_b).abs() < 0.0001);
    }

    #[test]
    fn lattice_phase_is_preserved_under_pan_and_zoom() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_max(
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::pos2(800.0, 600.0),
        );
        let world_point = eframe::egui::pos2(222.0, -118.0);
        let world_step = background_world_step(-1);

        state.pan = eframe::egui::vec2(123.0, -41.0);
        state.zoom = 1.9;
        let screen_origin = state.world_to_screen(eframe::egui::Pos2::ZERO, rect);
        let screen_point = state.world_to_screen(world_point, rect);
        let phase_x = lattice_phase_screen(
            screen_origin.x,
            screen_point.x,
            background_screen_step(-1, state.zoom),
        );
        let phase_y = lattice_phase_screen(
            screen_origin.y,
            screen_point.y,
            background_screen_step(-1, state.zoom),
        );

        assert!((phase_x - lattice_phase_world(world_point.x, world_step)).abs() < 0.0001);
        assert!((phase_y - lattice_phase_world(world_point.y, world_step)).abs() < 0.0001);
    }

    #[test]
    fn background_lod_settles_inside_spacing_band() {
        for zoom in [0.12, 0.33, 0.9, 1.0, 1.8, 4.2, 12.0] {
            let level = settled_background_lod_level(0, zoom);
            let screen_step = background_screen_step(level, zoom);
            assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
            assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
        }
    }

    #[test]
    fn background_lod_has_hysteresis_near_thresholds() {
        let level_after_zoom_out = settled_background_lod_level(0, 0.70);
        assert_eq!(level_after_zoom_out, 0);

        let level_after_more_zoom_out = settled_background_lod_level(level_after_zoom_out, 0.60);
        assert_eq!(level_after_more_zoom_out, 1);

        let level_after_small_recover =
            settled_background_lod_level(level_after_more_zoom_out, 0.76);
        assert_eq!(level_after_small_recover, 1);

        let level_after_large_zoom_in =
            settled_background_lod_level(level_after_small_recover, 1.80);
        assert!(level_after_large_zoom_in < level_after_small_recover);
        let screen_step = background_screen_step(level_after_large_zoom_in, 1.80);
        assert!(screen_step >= super::BACKGROUND_MIN_SCREEN_STEP);
        assert!(screen_step <= super::BACKGROUND_MAX_SCREEN_STEP);
    }

    #[test]
    fn background_lod_layers_crossfade_near_zoom_thresholds() {
        let zoom_near_zoom_out_switch =
            (super::BACKGROUND_MIN_SCREEN_STEP + 12.0) / super::BACKGROUND_BASE_WORLD_STEP;
        let layers = background_lod_layers(0, zoom_near_zoom_out_switch);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].level, 1);
        assert_eq!(layers[1].level, 0);
        assert!((layers[0].alpha - 0.5).abs() < 0.001);
        assert!((layers[1].alpha - 0.5).abs() < 0.001);

        let zoom_near_zoom_in_switch =
            (super::BACKGROUND_MAX_SCREEN_STEP - 12.0) / super::BACKGROUND_BASE_WORLD_STEP;
        let layers = background_lod_layers(0, zoom_near_zoom_in_switch);
        assert_eq!(layers.len(), 2);
        assert_eq!(layers[0].level, -1);
        assert_eq!(layers[1].level, 0);
        assert!((layers[0].alpha - 0.5).abs() < 0.001);
        assert!((layers[1].alpha - 0.5).abs() < 0.001);
    }

    #[test]
    fn horizontal_and_vertical_layouts_use_requested_gap() {
        let items = vec![
            layout_item(1, 80.0, 40.0),
            layout_item(2, 60.0, 40.0),
            layout_item(3, 40.0, 40.0),
        ];
        let gap = 18.0;

        let (horizontal, _, _) = horizontal_layout(&items, gap).unwrap();
        let (vertical, _, _) = vertical_layout(&items, gap).unwrap();

        assert_eq!(center_gap_x(&items, &horizontal, 1, 2), gap);
        assert_eq!(center_gap_y(&items, &vertical, 1, 2), gap);
    }

    #[test]
    fn compact_layout_respects_gap() {
        let items = vec![
            layout_item(1, 80.0, 80.0),
            layout_item(2, 80.0, 60.0),
            layout_item(3, 60.0, 80.0),
            layout_item(4, 60.0, 60.0),
        ];
        let gap = 12.0;

        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(rectangles_respect_gap(left_rect, right_rect, gap));
            }
        }
    }

    #[test]
    fn compact_layout_is_more_square_than_row_or_column_extremes() {
        let items = vec![
            layout_item(1, 180.0, 90.0),
            layout_item(2, 140.0, 80.0),
            layout_item(3, 110.0, 160.0),
            layout_item(4, 90.0, 110.0),
            layout_item(5, 70.0, 70.0),
        ];
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let (_, row_width, row_height) = horizontal_layout(&items, gap).unwrap();
        let (_, column_width, column_height) = vertical_layout(&items, gap).unwrap();

        let compact_ratio = aspect_ratio(compact_width, compact_height);
        let row_ratio = aspect_ratio(row_width, row_height);
        let column_ratio = aspect_ratio(column_width, column_height);

        assert!(compact_ratio < row_ratio);
        assert!(compact_ratio < column_ratio);
    }

    #[test]
    fn compact_layout_is_deterministic_for_mixed_sizes() {
        let items = hole_prone_items();
        let gap = 12.0;

        let first = best_compact_layout(&items, gap).unwrap();
        let second = best_compact_layout(&items, gap).unwrap();

        assert_eq!(first.1, second.1);
        assert_eq!(first.2, second.2);
        assert_eq!(first.0.len(), second.0.len());
        for ((first_id, first_pos), (second_id, second_pos)) in first.0.iter().zip(second.0.iter())
        {
            assert_eq!(first_id, second_id);
            assert!((first_pos.x - second_pos.x).abs() < 0.001);
            assert!((first_pos.y - second_pos.y).abs() < 0.001);
        }
    }

    #[test]
    fn compact_layout_has_smaller_area_than_legacy_skyline_on_hole_prone_fixture() {
        let items = hole_prone_items();
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let (_, legacy_width, legacy_height) = legacy_best_compact_layout(&items, gap).unwrap();

        assert!(compact_width * compact_height < legacy_width * legacy_height);
    }

    #[test]
    fn compact_layout_has_no_overlaps_for_hole_prone_fixture() {
        let items = hole_prone_items();
        let gap = 12.0;
        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(!left_rect.intersects(right_rect));
            }
        }
    }

    #[test]
    fn compact_layout_handles_larger_mixed_selection() {
        let mut items = Vec::new();
        let base = [
            (300.0, 520.0),
            (180.0, 260.0),
            (160.0, 260.0),
            (150.0, 220.0),
            (140.0, 180.0),
            (220.0, 140.0),
        ];
        for index in 0..48 {
            let (width, height) = base[index % base.len()];
            items.push(layout_item(index as i64 + 1, width, height));
        }

        let gap = 12.0;
        let (placements, _, _) = best_compact_layout(&items, gap).unwrap();

        assert_eq!(placements.len(), items.len());
        for left in &items {
            for right in &items {
                if left.id >= right.id {
                    continue;
                }
                let left_rect = placement_rect(&items, &placements, left.id);
                let right_rect = placement_rect(&items, &placements, right.id);
                assert!(rectangles_respect_gap(left_rect, right_rect, gap));
            }
        }
    }

    #[test]
    fn context_click_on_unselected_image_selects_it_before_requesting_menu() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(400.0, 300.0),
        );
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 40, 40, 1.0, eframe::egui::pos2(0.0, 0.0));
            push_test_image(
                ui,
                &mut state,
                2,
                40,
                40,
                1.0,
                eframe::egui::pos2(100.0, 0.0),
            );
        });
        state.select_items(&[1]);

        open_context_menu_at_pointer(
            &mut state,
            rect,
            rect.center() + eframe::egui::vec2(100.0, 0.0),
        );

        assert_eq!(state.selected_item_ids(), vec![2]);
        assert!(state.take_context_menu_request().is_some());
    }

    #[test]
    fn context_click_on_unselected_annotation_selects_it_before_requesting_menu() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(400.0, 300.0),
        );
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 40, 40, 1.0, eframe::egui::pos2(0.0, 0.0));
        });
        state.add_annotation(CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 80.0,
            start_y: 0.0,
            end_x: 160.0,
            end_y: 40.0,
            text: "note".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });
        state.select_items(&[1]);

        open_context_menu_at_pointer(
            &mut state,
            rect,
            rect.center() + eframe::egui::vec2(120.0, 20.0),
        );

        assert!(state.selected_item_ids().is_empty());
        assert_eq!(state.selected_annotation_ids(), vec![20]);
        assert!(state.take_context_menu_request().is_some());
    }

    #[test]
    fn context_click_uses_frontmost_mixed_canvas_stack_item() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(400.0, 300.0),
        );
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        });
        state.add_annotation(CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: -40.0,
            start_y: -40.0,
            end_x: 40.0,
            end_y: 40.0,
            text: "note".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });
        state.set_canvas_stack_order(vec![
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(1),
        ]);

        open_context_menu_at_pointer(&mut state, rect, rect.center());

        assert_eq!(state.selected_item_ids(), vec![1]);
        assert!(state.selected_annotation_ids().is_empty());
        assert!(state.take_context_menu_request().is_some());
    }

    #[test]
    fn canvas_paint_order_uses_unified_stack_order_for_mixed_items() {
        let mut state = CanvasState::default();
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
            push_test_image(ui, &mut state, 2, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        });
        state.add_annotation(CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: -40.0,
            start_y: -40.0,
            end_x: 40.0,
            end_y: 40.0,
            text: "note".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });
        state.set_canvas_stack_order(vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(2),
        ]);

        assert_eq!(
            unified_canvas_paint_order_for_state(&state),
            vec![
                CanvasStackItem::Placement(1),
                CanvasStackItem::Annotation(20),
                CanvasStackItem::Placement(2),
            ]
        );
    }

    #[test]
    fn context_click_inside_existing_mixed_selection_preserves_selection() {
        let mut state = CanvasState::default();
        let rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(400.0, 300.0),
        );
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 40, 40, 1.0, eframe::egui::pos2(0.0, 0.0));
        });
        state.add_annotation(CanvasAnnotation {
            id: 20,
            kind: CanvasAnnotationKind::Textbox,
            start_x: 80.0,
            start_y: 0.0,
            end_x: 160.0,
            end_y: 40.0,
            text: "note".to_owned(),
            text_size: default_text_size(),
            stroke_width: default_stroke_width(),
            color: default_annotation_color(),
            background_enabled: false,
            archived: false,
        });
        state.select_content(&[1], &[20]);

        open_context_menu_at_pointer(&mut state, rect, rect.center());

        assert_eq!(state.selected_item_ids(), vec![1]);
        assert_eq!(state.selected_annotation_ids(), vec![20]);
        assert!(state.take_context_menu_request().is_some());
    }

    fn layout_item(id: i64, width: f32, height: f32) -> SelectedLayoutItem {
        SelectedLayoutItem {
            id,
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn center_gap_x(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        left_id: i64,
        right_id: i64,
    ) -> f32 {
        let left = placement_rect(items, placements, left_id);
        let right = placement_rect(items, placements, right_id);
        right.left() - left.right()
    }

    fn center_gap_y(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        top_id: i64,
        bottom_id: i64,
    ) -> f32 {
        let top = placement_rect(items, placements, top_id);
        let bottom = placement_rect(items, placements, bottom_id);
        bottom.top() - top.bottom()
    }

    fn placement_rect(
        items: &[SelectedLayoutItem],
        placements: &[(i64, eframe::egui::Pos2)],
        item_id: i64,
    ) -> eframe::egui::Rect {
        let item = items.iter().find(|item| item.id == item_id).unwrap();
        let center = placements
            .iter()
            .find(|(placement_id, _)| *placement_id == item_id)
            .map(|(_, center)| *center)
            .unwrap();
        eframe::egui::Rect::from_center_size(center, eframe::egui::vec2(item.width, item.height))
    }

    fn aspect_ratio(width: f32, height: f32) -> f32 {
        width.max(height) / width.min(height).max(1.0)
    }

    fn hole_prone_items() -> Vec<SelectedLayoutItem> {
        vec![
            layout_item(1, 300.0, 520.0),
            layout_item(2, 180.0, 260.0),
            layout_item(3, 160.0, 260.0),
            layout_item(4, 150.0, 220.0),
            layout_item(5, 150.0, 220.0),
            layout_item(6, 140.0, 180.0),
            layout_item(7, 140.0, 180.0),
        ]
    }

    fn rectangles_respect_gap(
        left: eframe::egui::Rect,
        right: eframe::egui::Rect,
        gap: f32,
    ) -> bool {
        left.right() + gap <= right.left()
            || right.right() + gap <= left.left()
            || left.bottom() + gap <= right.top()
            || right.bottom() + gap <= left.top()
    }
}
