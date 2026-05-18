use std::sync::{Arc, Mutex};

mod document;
mod geometry;
pub(crate) mod guide_snap;
mod interaction;
mod render;
mod state;

use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasBackground,
    CanvasImageLod, CanvasTextSize, Placement,
};
use eframe::egui;
use interaction::{
    handle_input, handle_viewport_navigation_input, step_mouse_wheel_zoom_animation,
};
use render::{
    annotation_style_toolbar_rect, editing_textbox_toolbar_rect, paint_annotation_draft,
    paint_background, paint_canvas_items, paint_crop_marquee, paint_guide_snap_indicators,
    paint_marquee, paint_placement_preview_slots, paint_selection_bounds,
    show_annotation_style_toolbar, show_textbox_editor,
};
use state::{CanvasDocumentState, CanvasInteractionState, CanvasRenderState};

pub(crate) type SharedRgba = Arc<[u8]>;

const MIN_CANVAS_ZOOM: f32 = 0.01;
const MAX_CANVAS_ZOOM: f32 = 64.0;
const MIN_IMAGE_WORLD_SIZE: f32 = 24.0;
const IMAGE_CULL_MARGIN_SCREEN: f32 = 128.0;
const ANNOTATION_CULL_MARGIN_SCREEN: f32 = 128.0;
const IMAGE_RESIZE_HIT_RADIUS: f32 = 14.0;
const IMAGE_ROTATE_HANDLE_OFFSET_SCREEN: f32 = 18.0;
const IMAGE_ROTATE_HANDLE_RADIUS_SCREEN: f32 = 7.0;
const IMAGE_ROTATE_HANDLE_HIT_RADIUS_SCREEN: f32 = 13.0;
const SELECTION_BORDER_REAPPEAR_SECONDS: f64 = 2.0;
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
    pub(crate) layout_gap: f32,
    rotation_snap_degrees: f32,
    pointer_mode: crate::data::PointerMode,
    pub(crate) document: CanvasDocumentState,
    pub(crate) interaction: CanvasInteractionState,
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
    pub updated_placements: Vec<crate::data::PlacementState>,
    pub updated_annotations: Vec<CanvasAnnotation>,
    pub archived_annotation_ids: Vec<i64>,
    pub stack_order: Option<Vec<crate::data::CanvasStackItem>>,
}

impl CanvasDocumentChanges {
    pub fn has_changes(&self) -> bool {
        !self.updated_placements.is_empty()
            || !self.updated_annotations.is_empty()
            || !self.archived_annotation_ids.is_empty()
            || self.stack_order.is_some()
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
    #[allow(dead_code)]
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
pub(crate) struct RenderedPlacement {
    pub(crate) placement: Placement,
    texture: CanvasTexture,
    loaded_lod: CanvasImageLod,
    animation: Option<AnimatedCanvasImage>,
}

#[derive(Clone)]
struct PendingCropPreview {
    image: RenderedPlacement,
    rgba: SharedRgba,
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
    Unloaded {
        size: [usize; 2],
    },
}

impl CanvasTexture {
    fn id(&self) -> egui::TextureId {
        match self {
            Self::Managed(texture) => texture.id(),
            Self::Native(texture) => texture.texture_id(),
            Self::Svg { .. } => egui::TextureId::Managed(0),
            Self::Unloaded { .. } => egui::TextureId::Managed(0),
        }
    }

    fn size(&self) -> [usize; 2] {
        match self {
            Self::Managed(texture) => texture.size(),
            Self::Native(texture) => texture.size(),
            Self::Svg { size, .. } => *size,
            Self::Unloaded { size } => *size,
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
pub(crate) struct SelectedLayoutItem {
    pub(crate) id: i64,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Clone)]
struct ImageScaleStart {
    placement_id: i64,
    center_world: egui::Pos2,
    base_visual_size: egui::Vec2,
    start_scale: f32,
    min_scale: f32,
}

#[derive(Clone)]
pub(crate) struct DragState {
    pub(crate) pointer_start_world: egui::Pos2,
    pub(crate) starting_items: Vec<SelectedLayoutItem>,
    pub(crate) snap_preview_rect_world: Option<egui::Rect>,
    pub(crate) started_at_seconds: f64,
    pub(crate) last_movement_seconds: f64,
    pub(crate) last_world_delta: egui::Vec2,
    pub(crate) moved: bool,
}

#[derive(Clone)]
struct AnnotationDragState {
    annotation_ids: Vec<i64>,
    pointer_start_world: egui::Pos2,
    pointer_start_screen: egui::Pos2,
    starting_points: Vec<(i64, egui::Pos2, egui::Pos2)>,
}

#[derive(Clone)]
enum ImageTransformState {
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
        starting_items: Vec<ImageScaleStart>,
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
struct PlacementPreviewSlot {
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
    placement_id: i64,
    start_screen: egui::Pos2,
    current_screen: egui::Pos2,
}

#[derive(Clone, Debug)]
struct AnimatedCanvasFrame {
    rgba: SharedRgba,
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
    pub placement_id: i64,
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
            started_at_seconds: 0.0,
            last_movement_seconds: 0.0,
            last_world_delta: egui::Vec2::ZERO,
            moved: false,
        };
        let snap_preview_rect = crate::placement::movement::compute_drag_snap_preview(
            self,
            rect,
            &drag_state,
            world_delta,
        );
        let snap_offset = snap_preview_rect
            .and_then(|snap_rect| {
                crate::placement::movement::layout_bounds_with_offset(
                    &drag_state.starting_items,
                    world_delta,
                )
                .map(|bounds| snap_rect.center() - bounds.center())
            })
            .unwrap_or(egui::Vec2::ZERO);
        for item in &drag_state.starting_items {
            if let Some(image) = self
                .document
                .rendered_placements
                .iter_mut()
                .find(|image| image.placement.state.id == item.id)
            {
                image.placement.state.center.x = item.x + world_delta.x + snap_offset.x;
                image.placement.state.center.y = item.y + world_delta.y + snap_offset.y;
            }
        }
        snap_preview_rect.is_some()
    }
}

pub(crate) fn rotated_visual_size(width: f32, height: f32, rotation: f32) -> egui::Vec2 {
    let sin = rotation.sin().abs();
    let cos = rotation.cos().abs();
    egui::vec2(width * cos + height * sin, width * sin + height * cos)
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

impl ImagePassResources {
    fn new(device: &eframe::wgpu::Device, target_format: eframe::wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(eframe::wgpu::ShaderModuleDescriptor {
            label: Some("snappyref_canvas_image_shader"),
            source: eframe::wgpu::ShaderSource::Wgsl(IMAGE_PASS_SHADER.into()),
        });
        let texture_bind_group_layout =
            device.create_bind_group_layout(&eframe::wgpu::BindGroupLayoutDescriptor {
                label: Some("snappyref_canvas_image_bind_group_layout"),
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
            label: Some("snappyref_canvas_image_sampler"),
            mag_filter: eframe::wgpu::FilterMode::Linear,
            min_filter: eframe::wgpu::FilterMode::Linear,
            mipmap_filter: eframe::wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout =
            device.create_pipeline_layout(&eframe::wgpu::PipelineLayoutDescriptor {
                label: Some("snappyref_canvas_image_pipeline_layout"),
                bind_group_layouts: &[Some(&texture_bind_group_layout)],
                immediate_size: 0,
            });
        let vertex_attributes = eframe::wgpu::vertex_attr_array![0 => Float32x2];
        let instance_attributes = eframe::wgpu::vertex_attr_array![1 => Float32x2, 2 => Float32x2, 3 => Float32x2, 4 => Float32x2];
        let pipeline = device.create_render_pipeline(&eframe::wgpu::RenderPipelineDescriptor {
            label: Some("snappyref_canvas_image_pipeline"),
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
                label: Some("snappyref_canvas_image_vertices"),
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
                label: Some("snappyref_canvas_image_instances"),
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
    allow_lod_promotion: bool,
    input_policy: CanvasInputPolicy,
    debug_labels: bool,
) -> CanvasFrameMetrics {
    let now_seconds = ui.input(|input| input.time);
    if step_mouse_wheel_zoom_animation(state, now_seconds) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(8));
    }
    let toolbar_rect = editing_textbox_toolbar_rect(ui, state, rect)
        .or_else(|| annotation_style_toolbar_rect(ui, state, rect));
    let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
    match input_policy {
        CanvasInputPolicy::Full => {
            handle_input(ui, rect, input_rect, toolbar_rect, state, &response);
        }
        CanvasInputPolicy::ViewportOnly => {
            clear_canvas_edit_interaction(state);
            handle_viewport_navigation_input(ui, rect, input_rect, toolbar_rect, state, &response);
        }
        CanvasInputPolicy::Blocked => {
            clear_canvas_edit_interaction(state);
        }
    }
    state.prune_cached_pixels_to_selected_items();

    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, crate::ui::theme::BACKGROUND);
    paint_background(&painter, rect, state);
    paint_placement_preview_slots(&painter, rect, state);
    let image_pass = paint_canvas_items(
        &painter,
        ui,
        rect,
        state,
        render_state,
        allow_lod_promotion,
        debug_labels,
    );
    paint_selection_bounds(&painter, rect, state);
    paint_guide_snap_indicators(&painter, rect, state);
    paint_crop_marquee(&painter, rect, state);
    paint_annotation_draft(&painter, rect, state);
    paint_marquee(&painter, state);
    show_annotation_style_toolbar(ui, rect, state);
    show_textbox_editor(ui, rect, state);
    CanvasFrameMetrics { image_pass }
}

fn clear_canvas_edit_interaction(state: &mut CanvasState) {
    state.interaction.drag_state = None;
    state.interaction.marquee_state = None;
    state.interaction.crop_state = None;
    state.interaction.annotation_draft_state = None;
}

fn rotation_handle_key_held(ctx: &egui::Context) -> bool {
    !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_down(egui::Key::R))
}

pub fn paint_perf_report() -> PaintPerfReport {
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(960.0, 640.0));
    let pan = egui::Vec2::ZERO;
    let zoom = 1.0;
    let placements = (0..400)
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
            crate::data::Placement::loaded(
                crate::data::PlacementState {
                    id: index as i64 + 1,
                    asset_id: Some(index as i64 + 1),
                    center: egui::pos2(x, y),
                    scale: 1.0,
                    rotation: 0.0,
                    z_index: index as i32,
                    archived: false,
                },
                crate::data::ImageAsset {
                    id: index as i64 + 1,
                    original_path: std::path::PathBuf::new(),
                    filename: format!("perf-{index}.png"),
                    mime_type: "image/png".to_owned(),
                    width: 512,
                    height: 512,
                    file_size: 0,
                    imported_at: 0,
                },
            )
        })
        .collect::<Vec<_>>();

    let legacy_started_at = std::time::Instant::now();
    let legacy_stats = render::image_paint_stats_for_records(&placements, rect, pan, zoom, false);
    let legacy_visibility_pass_ms = legacy_started_at.elapsed().as_secs_f64() * 1000.0;
    let current_started_at = std::time::Instant::now();
    let current_stats = render::image_paint_stats_for_records(&placements, rect, pan, zoom, true);
    let current_visibility_pass_ms = current_started_at.elapsed().as_secs_f64() * 1000.0;
    let current_texture_groups =
        render::image_texture_group_count_for_records(&placements, rect, pan, zoom);

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
        handle_textbox_primary_press, hit_test_canvas_item, hit_test_image_resize_handle,
        hit_test_textbox_resize_handle, image_screen_rect, moved_textbox_rect,
        normalized_textbox_rect, open_context_menu_at_pointer, pending_annotation_from_draft,
        resized_image_transform, resized_textbox_rect, textbox_screen_rect,
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
        AnnotationDraftState, CanvasCropRequest, CanvasInputPolicy, CanvasState, CanvasTexture,
        FOCUS_SELECTION_PADDING_SCREEN, ImageResizeHandle, MIN_CANVAS_ZOOM, PixelCropRect,
        RenderedPlacement, SelectedLayoutItem, TEXTBOX_EDITOR_TOOLBAR_CLEARANCE,
        TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN, TextboxPointerRegion, TextboxResizeHandle,
        rotated_visual_size,
    };
    use crate::data::{
        CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasImageLod,
        CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, ContentResolutionSource, ImageAsset,
        Placement, PlacementContent, PlacementLifecycle, PlacementState,
    };
    use crate::placement::arrangement::{
        SettleDirection, balanced_grid_layout_for_sizes, best_compact_layout,
        compact_layout_for_sizes, directional_settle_layout, horizontal_layout,
        layout_bounds_for_items, vertical_layout,
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
        placement_id: i64,
        width: u32,
        height: u32,
        scale: f32,
        center: eframe::egui::Pos2,
    ) {
        let texture = ui.ctx().load_texture(
            format!("test-image-{placement_id}"),
            eframe::egui::ColorImage::filled([4, 4], eframe::egui::Color32::WHITE),
            eframe::egui::TextureOptions::LINEAR,
        );
        state.document.rendered_placements.push(RenderedPlacement {
            placement: Placement::loaded(
                PlacementState {
                    id: placement_id,
                    asset_id: Some(placement_id),
                    center: eframe::egui::pos2(center.x, center.y),
                    scale,
                    rotation: 0.0,
                    z_index: 0,
                    archived: false,
                },
                ImageAsset {
                    id: placement_id,
                    original_path: PathBuf::from(format!("image-{placement_id}.png")),
                    filename: format!("image-{placement_id}.png"),
                    mime_type: "image/png".to_owned(),
                    width,
                    height,
                    file_size: 0,
                    imported_at: 0,
                },
            ),
            texture: CanvasTexture::Managed(texture),
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        });
    }

    #[test]
    fn viewport_only_input_zooms_without_preview_state() {
        let ctx = eframe::egui::Context::default();
        let mut state = CanvasState::default();
        state.set_pointer_mode(crate::data::PointerMode::Trackpad);
        let screen_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 600.0),
        );

        let _ = ctx.run_ui(
            eframe::egui::RawInput {
                events: vec![
                    eframe::egui::Event::PointerMoved(screen_rect.center()),
                    eframe::egui::Event::Zoom(0.5),
                ],
                ..Default::default()
            },
            |ui| {
                super::show(
                    ui,
                    screen_rect,
                    screen_rect,
                    &mut state,
                    None,
                    false,
                    CanvasInputPolicy::ViewportOnly,
                    false,
                );
            },
        );

        assert!(state.viewport_state().camera_zoom < 1.0);
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

    fn test_record(placement_id: i64, asset_id: i64, width: u32, height: u32) -> Placement {
        Placement::loaded(
            PlacementState {
                id: placement_id,
                asset_id: Some(asset_id),
                center: eframe::egui::Pos2::ZERO,
                scale: 1.0,
                rotation: 0.0,
                z_index: placement_id as i32,
                archived: false,
            },
            ImageAsset {
                id: asset_id,
                original_path: PathBuf::from(format!("image-{asset_id}.png")),
                filename: format!("image-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height,
                file_size: 0,
                imported_at: 0,
            },
        )
    }

    fn unloaded_test_record(
        placement_id: i64,
        content: PlacementContent,
        visual_width: f32,
        visual_height: f32,
        scale: f32,
        center: eframe::egui::Pos2,
    ) -> Placement {
        let mut placement = Placement {
            state: PlacementState {
                id: placement_id,
                asset_id: None,
                center,
                scale,
                rotation: 0.0,
                z_index: placement_id as i32,
                archived: false,
            },
            asset: None,
            content: content.clone(),
            lifecycle: PlacementLifecycle::Committed,
            visual_size: eframe::egui::vec2(visual_width, visual_height),
        };
        placement.content = content;
        placement.lifecycle = PlacementLifecycle::Committed;
        placement
    }

    fn native_loaded_item(
        placement: Placement,
        texture: Arc<crate::jobs::NativeTextureHandle>,
    ) -> crate::jobs::PreparedPlacementImage {
        crate::jobs::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            preview_rgba: vec![
                255;
                placement
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .width as usize
                    * placement
                        .asset
                        .as_ref()
                        .expect("loaded placement asset")
                        .height as usize
                    * 4
            ],
            preview_width: placement
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .width,
            preview_height: placement
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .height,
            animated_preview: None,
            preview_texture: crate::jobs::ImportedPreviewTexture::Native(texture),
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            placement,
            origin: crate::jobs::ImportOrigin::Existing,
            trace: None,
        }
    }

    fn animated_loaded_item(
        _ui: &eframe::egui::Ui,
        placement: Placement,
    ) -> crate::jobs::PreparedPlacementImage {
        let first_frame = vec![0, 0, 255, 255];
        let second_frame = vec![0, 255, 0, 255];
        crate::jobs::PreparedPlacementImage {
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
            loaded_lod: CanvasImageLod::Working,
            existing_load_diagnostics: None,
            placement,
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

        state.add_prepared_placement_image(&ctx, native_loaded_item(first, handle.clone()));
        state
            .add_loaded_reference_placement(test_record(2, 1, 4, 4))
            .unwrap();
        drop(handle);

        state.delete_placements(&[1]);
        assert_eq!(cleanup_count.load(Ordering::SeqCst), 0);

        state.delete_placements(&[2]);
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

        state.add_prepared_placement_image(
            &ctx,
            native_loaded_item(test_record(1, 1, 4, 4), old_texture.clone()),
        );
        state
            .add_loaded_reference_placement(test_record(2, 1, 4, 4))
            .unwrap();
        drop(old_texture);

        state
            .apply_cropped_item(
                &ctx,
                crate::jobs::PreparedCropPreview {
                    placement_id: 1,
                    stored_path: PathBuf::from("crop.png"),
                    width: 2,
                    height: 2,
                    file_size: 0,
                    preview_rgba: vec![200; 16],
                    preview_width: 2,
                    preview_height: 2,
                    preview_texture: crate::jobs::ImportedPreviewTexture::Native(
                        new_texture.clone(),
                    ),
                    loaded_lod: CanvasImageLod::Working,
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
                PlacementState {
                    id: 1,
                    asset_id: Some(2),
                    center: eframe::egui::pos2(0.0, 0.0),
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

        state.delete_placements(&[2]);
        assert_eq!(old_cleanup.load(Ordering::SeqCst), 1);
        assert_eq!(new_cleanup.load(Ordering::SeqCst), 0);

        state.delete_placements(&[1]);
        assert_eq!(new_cleanup.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reference_items_preserve_gif_animation_after_source_item_is_removed() {
        let mut state = CanvasState::default();

        eframe::egui::__run_test_ui(|ui| {
            state.add_prepared_placement_image(
                ui.ctx(),
                animated_loaded_item(ui, test_record(1, 1, 1, 1)),
            );
        });
        state
            .add_loaded_reference_placement(test_record(2, 1, 1, 1))
            .unwrap();

        assert!(state.document.rendered_placements[1].animation.is_some());
        assert_eq!(
            state.advance_animated_image(2, 0.15).unwrap().0.as_ref(),
            &[0, 255, 0, 255]
        );

        state.delete_placements(&[1]);

        assert_eq!(
            state.advance_animated_image(2, 0.25).unwrap().0.as_ref(),
            &[0, 0, 255, 255]
        );
        assert!(
            state
                .document
                .rendered_placements
                .iter()
                .any(|image| image.placement.state.id == 2 && image.animation.is_some())
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

        let item = PlacementState {
            id: 1,
            asset_id: Some(1),
            center: eframe::egui::pos2(200.0, 120.0),
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

        assert_eq!(merged_item.asset_id, Some(2));
        assert_eq!(merged_item.center.x, 150.0);
        assert_eq!(merged_item.center.y, 110.0);
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
            state.select_placements(&[1, 2]);

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
    fn unloaded_placements_use_visual_bounds_for_layout_hit_test_and_delete() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            let canvas_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(800.0, 600.0),
            );
            let texture = ui.ctx().load_texture(
                "unloaded-placement-test",
                eframe::egui::ColorImage::filled([1, 1], eframe::egui::Color32::WHITE),
                eframe::egui::TextureOptions::LINEAR,
            );
            state.document.rendered_placements.push(RenderedPlacement {
                placement: unloaded_test_record(
                    101,
                    PlacementContent::Loading {
                        source: ContentResolutionSource::Unknown {
                            description: "test".to_owned(),
                        },
                        status_message: None,
                    },
                    120.0,
                    80.0,
                    2.0,
                    eframe::egui::pos2(10.0, 20.0),
                ),
                texture: CanvasTexture::Managed(texture),
                loaded_lod: CanvasImageLod::Working,
                animation: None,
            });
            state
                .document
                .stack_order
                .push(CanvasStackItem::Placement(101));
            state.select_placements(&[101]);

            let layout_items = state.selected_layout_items();
            assert_eq!(layout_items.len(), 1);
            assert_eq!(layout_items[0].id, 101);
            assert_eq!(layout_items[0].x, 10.0);
            assert_eq!(layout_items[0].y, 20.0);
            assert_eq!(layout_items[0].width, 240.0);
            assert_eq!(layout_items[0].height, 160.0);
            assert_eq!(
                state.selected_bounds_world().unwrap(),
                eframe::egui::Rect::from_center_size(
                    eframe::egui::pos2(10.0, 20.0),
                    eframe::egui::vec2(240.0, 160.0),
                )
            );
            assert_eq!(
                hit_test_canvas_item(&state, canvas_rect, canvas_rect.center()),
                Some(super::CanvasHit::Placement(101))
            );
            assert_eq!(state.delete_selected_images(), vec![101]);
            assert!(state.placement_for_id(101).is_none());
        });
    }

    #[test]
    fn provisional_placements_are_filtered_from_document_changes() {
        let mut state = CanvasState::default();
        let mut placement = unloaded_test_record(
            -1,
            PlacementContent::Loading {
                source: ContentResolutionSource::SearchAsset { asset_id: 42 },
                status_message: Some("Loading".to_owned()),
            },
            120.0,
            80.0,
            1.0,
            eframe::egui::pos2(10.0, 20.0),
        );
        placement.lifecycle = PlacementLifecycle::Provisional;
        state.add_provisional_unloaded_item(placement);
        state.select_placements(&[-1]);
        state.center_selected_placements_on(eframe::egui::pos2(40.0, 60.0));

        let changes = state.take_document_changes();

        assert!(changes.updated_placements.is_empty());
        assert!(changes.stack_order.is_none());
    }

    #[test]
    fn cancel_removes_provisional_placements_without_archiving_rows() {
        let mut state = CanvasState::default();
        let mut placement = unloaded_test_record(
            -1,
            PlacementContent::Loading {
                source: ContentResolutionSource::SearchAsset { asset_id: 42 },
                status_message: Some("Loading".to_owned()),
            },
            120.0,
            80.0,
            1.0,
            eframe::egui::pos2(10.0, 20.0),
        );
        placement.lifecycle = PlacementLifecycle::Provisional;
        state.add_provisional_unloaded_item(placement);
        state.select_placements(&[-1]);

        let removed = state.remove_provisional_placements();
        let changes = state.take_document_changes();

        assert_eq!(removed, vec![-1]);
        assert!(state.placement_for_id(-1).is_none());
        assert!(changes.updated_placements.is_empty());
        assert!(changes.archived_annotation_ids.is_empty());
    }

    #[test]
    fn placement_preview_slots_replace_by_queue_id_and_remove_cleanly() {
        let mut state = CanvasState::default();
        state.add_placement_preview_slot(
            7,
            eframe::egui::pos2(10.0, 20.0),
            eframe::egui::vec2(100.0, 80.0),
            "first".to_owned(),
        );
        state.add_placement_preview_slot(
            7,
            eframe::egui::pos2(30.0, 40.0),
            eframe::egui::vec2(120.0, 90.0),
            "updated".to_owned(),
        );

        assert_eq!(state.placement_preview_slot_count(), 1);
        state.remove_placement_preview_slot(7);
        assert_eq!(state.placement_preview_slot_count(), 0);
    }

    #[test]
    fn hover_preview_slots_replace_and_clear_without_touching_queued_placeholders() {
        let mut state = CanvasState::default();
        state.add_placement_preview_slot(
            1,
            eframe::egui::pos2(0.0, 0.0),
            eframe::egui::vec2(100.0, 80.0),
            "queued".to_owned(),
        );

        state.set_hover_preview_slots(vec![
            (
                eframe::egui::pos2(10.0, 10.0),
                eframe::egui::vec2(50.0, 50.0),
            ),
            (
                eframe::egui::pos2(70.0, 10.0),
                eframe::egui::vec2(50.0, 50.0),
            ),
        ]);
        assert_eq!(state.hover_preview_slot_count(), 2);
        assert_eq!(state.placement_preview_slot_count(), 1);

        state.set_hover_preview_slots(vec![(
            eframe::egui::pos2(20.0, 20.0),
            eframe::egui::vec2(40.0, 40.0),
        )]);
        assert_eq!(state.hover_preview_slot_count(), 1);

        state.clear_hover_preview_slots();
        assert_eq!(state.hover_preview_slot_count(), 0);
        assert_eq!(state.placement_preview_slot_count(), 1);
    }

    #[test]
    fn clearing_hover_preview_slots_also_clears_guide_indicators() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            push_test_image(
                ui,
                &mut state,
                1,
                100,
                100,
                1.0,
                eframe::egui::pos2(0.0, 0.0),
            );

            state.compact_guided_preview_placeholders(
                &[eframe::egui::vec2(100.0, 100.0)],
                eframe::egui::pos2(102.0, 0.0),
                eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(800.0, 600.0),
                ),
                24.0,
            );

            assert_eq!(state.guide_snap_indicator_count(), 1);
            state.clear_hover_preview_slots();
            assert_eq!(state.guide_snap_indicator_count(), 0);
        });
    }

    #[test]
    fn compact_guided_preview_placeholders_snap_as_one_batch() {
        eframe::egui::__run_test_ui(|ui| {
            let mut state = CanvasState::default();
            push_test_image(
                ui,
                &mut state,
                1,
                100,
                100,
                1.0,
                eframe::egui::pos2(0.0, 0.0),
            );

            let placeholders = state.compact_guided_preview_placeholders(
                &[
                    eframe::egui::vec2(100.0, 100.0),
                    eframe::egui::vec2(100.0, 100.0),
                ],
                eframe::egui::pos2(102.0, 0.0),
                eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(800.0, 600.0),
                ),
                24.0,
            );

            let bounds = placeholders
                .iter()
                .map(|(center, size)| eframe::egui::Rect::from_center_size(*center, *size))
                .reduce(|left, right| left.union(right))
                .expect("preview bounds");
            assert_eq!(placeholders.len(), 2);
            assert_eq!(bounds.left(), 50.0);
            assert_eq!(state.guide_snap_indicator_count(), 1);
        });
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
            let image = state.rendered_placement_by_id(21).unwrap();
            let image_rect = image_screen_rect(&state, canvas_rect, image);
            let pointer_pos = image_rect.right_bottom() + eframe::egui::vec2(8.0, 8.0);

            assert_eq!(
                hit_test_image_resize_handle(&state, canvas_rect, image, pointer_pos),
                Some(ImageResizeHandle::BottomRight)
            );
        });
    }

    #[test]
    fn image_resize_scales_proportionally_from_opposite_corner() {
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
        let anchor = eframe::egui::pos2(200.0, 190.0);
        let start_handle = eframe::egui::pos2(400.0, 290.0);
        let pointer_pos = eframe::egui::pos2(600.0, 390.0);

        let (center, scale) =
            resized_image_transform(&asset, 1.0, anchor, start_handle, pointer_pos);

        assert!((scale - 2.0).abs() < 0.01);
        assert_eq!(center, eframe::egui::pos2(400.0, 290.0));
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

        let (_, scale) = resized_image_transform(
            &asset,
            1.0,
            eframe::egui::pos2(100.0, 100.0),
            eframe::egui::pos2(500.0, 300.0),
            eframe::egui::pos2(100.5, 100.5),
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
            state.select_placements(&[31, 32]);

            let changed = state.apply_target_area_to_selected_images(90_000.0);

            assert_eq!(changed, 2);
            for placement_id in [31, 32] {
                let image = state.rendered_placement_by_id(placement_id).unwrap();
                let area = image
                    .placement
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .width as f32
                    * image
                        .placement
                        .asset
                        .as_ref()
                        .expect("loaded placement asset")
                        .height as f32
                    * image.placement.state.scale
                    * image.placement.state.scale;
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
            state.select_placements(&[41, 42]);

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

        assert!(state.document.selected_placement_ids.is_empty());
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
        let mut item = PlacementState {
            id: 7,
            asset_id: Some(2),
            center: eframe::egui::pos2(12.0, 18.0),
            scale: 1.5,
            rotation: 0.25,
            z_index: 9,
            archived: false,
        };

        merge_cropped_item_fields(&mut item, 11, eframe::egui::vec2(80.0, 96.0));

        assert_eq!(item.asset_id, Some(11));
        assert_eq!(item.center.x, 132.0);
        assert_eq!(item.center.y, 162.0);
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
        state.document.rendered_placements.push(RenderedPlacement {
            placement: test_record(42, 42, 4, 4),
            texture: CanvasTexture::Native(handle),
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        });

        assert!(state.can_start_crop_on_placement(42));
        state.mark_crop_pending(42);
        assert!(!state.can_start_crop_on_placement(42));
        state.clear_crop_pending(42);
        assert!(state.can_start_crop_on_placement(42));
    }

    #[test]
    fn crop_guard_rejects_svg_items() {
        let mut state = super::CanvasState::default();
        let mut placement = test_record(42, 42, 4, 4);
        placement
            .asset
            .as_mut()
            .expect("loaded placement asset")
            .mime_type = "image/svg+xml".to_owned();
        state.document.rendered_placements.push(RenderedPlacement {
            placement,
            texture: CanvasTexture::Svg {
                uri: "memory://test.svg".to_owned(),
                bytes: Arc::<[u8]>::from(b"<svg viewBox='0 0 4 4'/>".as_slice()),
                size: [4, 4],
            },
            loaded_lod: CanvasImageLod::Working,
            animation: None,
        });

        assert!(!state.can_start_crop_on_placement(42));
    }

    #[test]
    fn pending_crop_preview_uses_cached_pixels_before_worker_finishes() {
        let ctx = eframe::egui::Context::default();
        let mut state = super::CanvasState::default();
        let placement = test_record(42, 42, 8, 8);
        let source_rgba = (0..16)
            .flat_map(|index| [index, 0, 0, 255])
            .collect::<Vec<_>>();
        state.add_prepared_placement_image(
            &ctx,
            crate::jobs::PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
                preview_rgba: source_rgba.clone(),
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        state
            .render
            .asset_rgba_cache
            .insert(42, source_rgba.clone().into());

        let request = CanvasCropRequest {
            placement_id: 42,
            source_asset: test_record(42, 42, 8, 8)
                .asset
                .expect("loaded placement asset"),
            crop_rect: PixelCropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            crop_offset_local: eframe::egui::vec2(2.0, 2.0),
        };

        state.apply_pending_crop_preview(&ctx, &request).unwrap();

        let preview = state.preview_for_placement_id(42).unwrap();
        assert_eq!(preview.texture_size, eframe::egui::vec2(2.0, 2.0));
        let placement = state.placement_for_id(42).unwrap();
        assert_eq!(
            (
                placement
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .width,
                placement
                    .asset
                    .as_ref()
                    .expect("loaded placement asset")
                    .height
            ),
            (4, 4)
        );
        assert_eq!(
            (placement.state.center.x, placement.state.center.y),
            (2.0, 2.0)
        );

        let original = state.snapshot_item_image(42).unwrap();
        assert_eq!(
            original
                .0
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .width,
            8
        );
        assert_eq!(original.1.as_ref(), source_rgba);
    }

    #[test]
    fn pending_crop_restore_snapshot_shares_warmed_pixels() {
        let ctx = eframe::egui::Context::default();
        let mut state = super::CanvasState::default();
        let placement = test_record(42, 42, 8, 8);
        let source_rgba = vec![255; 4 * 4 * 4];
        let shared_rgba = super::SharedRgba::from(source_rgba.clone());
        state.add_prepared_placement_image(
            &ctx,
            crate::jobs::PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
                preview_rgba: source_rgba,
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        state
            .render
            .asset_rgba_cache
            .insert(42, shared_rgba.clone());

        let request = CanvasCropRequest {
            placement_id: 42,
            source_asset: test_record(42, 42, 8, 8)
                .asset
                .expect("loaded placement asset"),
            crop_rect: PixelCropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            crop_offset_local: eframe::egui::vec2(2.0, 2.0),
        };

        state.apply_pending_crop_preview(&ctx, &request).unwrap();

        let pending = state.render.pending_crop_previews.get(&42).unwrap();
        assert!(Arc::ptr_eq(&pending.rgba, &shared_rgba));
        let original = state.snapshot_item_image(42).unwrap();
        assert!(Arc::ptr_eq(&original.1, &shared_rgba));
    }

    #[test]
    fn loaded_raster_does_not_eagerly_cache_cpu_pixels() {
        let ctx = eframe::egui::Context::default();
        let mut state = super::CanvasState::default();

        state.add_prepared_placement_image(
            &ctx,
            crate::jobs::PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement: test_record(42, 42, 8, 8),
                preview_rgba: vec![255; 4 * 4 * 4],
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );

        assert!(state.render.asset_rgba_cache.is_empty());
    }

    #[test]
    fn deselection_prunes_warmed_cpu_pixels() {
        let ctx = eframe::egui::Context::default();
        let mut state = super::CanvasState::default();

        state.add_prepared_placement_image(
            &ctx,
            crate::jobs::PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement: test_record(42, 42, 8, 8),
                preview_rgba: vec![255; 4 * 4 * 4],
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        state.select_content(&[42], &[]);
        state
            .render
            .asset_rgba_cache
            .insert(42, vec![255; 4 * 4 * 4].into());

        state.select_content(&[], &[]);

        assert!(state.render.asset_rgba_cache.is_empty());
    }

    #[test]
    fn final_crop_integration_after_pending_preview_applies_crop_offset_once() {
        let ctx = eframe::egui::Context::default();
        let mut state = super::CanvasState::default();
        let placement = test_record(42, 42, 8, 8);
        state.add_prepared_placement_image(
            &ctx,
            crate::jobs::PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement,
                preview_rgba: vec![255; 4 * 4 * 4],
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );
        state
            .render
            .asset_rgba_cache
            .insert(42, vec![255; 4 * 4 * 4].into());
        let request = CanvasCropRequest {
            placement_id: 42,
            source_asset: test_record(42, 42, 8, 8)
                .asset
                .expect("loaded placement asset"),
            crop_rect: PixelCropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            crop_offset_local: eframe::egui::vec2(2.0, 2.0),
        };
        state.apply_pending_crop_preview(&ctx, &request).unwrap();
        let cropped = crate::jobs::PreparedCropPreview {
            placement_id: 42,
            stored_path: std::path::PathBuf::from("cropped.png"),
            width: 4,
            height: 4,
            file_size: 0,
            preview_rgba: vec![128; 2 * 2 * 4],
            preview_width: 2,
            preview_height: 2,
            preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
            loaded_lod: CanvasImageLod::Overview,
            crop_offset_local: eframe::egui::vec2(2.0, 2.0),
        };
        let mut updated_item = state.projected_cropped_item(&cropped).unwrap();
        let mut asset = test_record(42, 99, 4, 4)
            .asset
            .expect("loaded placement asset");
        asset.original_path = std::path::PathBuf::from("cropped.png");
        updated_item.asset_id = Some(asset.id);

        state
            .apply_cropped_item(&ctx, cropped, asset, updated_item)
            .unwrap();

        let placement = state.placement_for_id(42).unwrap();
        assert_eq!(placement.state.asset_id, Some(99));
        assert_eq!(
            (placement.state.center.x, placement.state.center.y),
            (2.0, 2.0)
        );
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
    fn directional_settle_right_moves_sparse_rows_to_wall() {
        let items = vec![
            positioned_layout_item(1, 50.0, 50.0, 100.0, 40.0),
            positioned_layout_item(2, 170.0, 50.0, 100.0, 40.0),
            positioned_layout_item(3, 50.0, 160.0, 100.0, 40.0),
        ];
        let gap = 20.0;

        let placements =
            directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

        let top_right = placement_rect(&items, &placements, 2);
        let top_left = placement_rect(&items, &placements, 1);
        let bottom_lone = placement_rect(&items, &placements, 3);

        assert_eq!(top_right.right(), 220.0);
        assert_eq!(top_left.right() + gap, top_right.left());
        assert_eq!(bottom_lone.right(), 220.0);
    }

    #[test]
    fn directional_settle_uses_gap_expanded_cross_axis_blocking() {
        let items = vec![
            positioned_layout_item(1, 150.0, 50.0, 80.0, 40.0),
            positioned_layout_item(2, 20.0, 105.0, 80.0, 40.0),
        ];
        let gap = 20.0;

        let placements =
            directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

        let settled_first = placement_rect(&items, &placements, 1);
        let settled_second = placement_rect(&items, &placements, 2);
        assert_eq!(settled_first.right(), 190.0);
        assert_eq!(settled_second.right() + gap, settled_first.left());
    }

    #[test]
    fn directional_settle_grace_does_not_block_rows_at_exact_gap() {
        let items = vec![
            positioned_layout_item(1, 150.0, 50.0, 80.0, 40.0),
            positioned_layout_item(2, 20.0, 110.0, 80.0, 40.0),
        ];
        let gap = 20.0;

        let placements =
            directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();

        let settled_first = placement_rect(&items, &placements, 1);
        let settled_second = placement_rect(&items, &placements, 2);
        assert_eq!(settled_first.right(), 190.0);
        assert_eq!(settled_second.right(), 190.0);
    }

    #[test]
    fn directional_settle_keeps_target_edge_fixed_and_can_shrink_opposite_edge() {
        let items = vec![
            positioned_layout_item(1, 50.0, 50.0, 80.0, 40.0),
            positioned_layout_item(2, 250.0, 50.0, 80.0, 40.0),
        ];
        let original_bounds = layout_bounds_for_items(&items).unwrap();

        let placements =
            directional_settle_layout(&items, SettleDirection::Right, 20.0, 1.0).unwrap();
        let settled_items = placements
            .iter()
            .map(|(id, center)| {
                let item = items.iter().find(|item| item.id == *id).unwrap();
                positioned_layout_item(*id, center.x, center.y, item.width, item.height)
            })
            .collect::<Vec<_>>();
        let settled_bounds = layout_bounds_for_items(&settled_items).unwrap();

        assert_eq!(settled_bounds.right(), original_bounds.right());
        assert!(settled_bounds.left() > original_bounds.left());
    }

    #[test]
    fn directional_settle_layout_is_idempotent() {
        let items = vec![
            positioned_layout_item(1, 10.0, 50.0, 100.0, 40.0),
            positioned_layout_item(2, 170.0, 50.0, 100.0, 40.0),
            positioned_layout_item(3, 50.0, 160.0, 100.0, 40.0),
        ];
        let gap = 20.0;
        let first = directional_settle_layout(&items, SettleDirection::Right, gap, 1.0).unwrap();
        let settled_items = first
            .iter()
            .map(|(id, center)| {
                let item = items.iter().find(|item| item.id == *id).unwrap();
                positioned_layout_item(*id, center.x, center.y, item.width, item.height)
            })
            .collect::<Vec<_>>();

        let second =
            directional_settle_layout(&settled_items, SettleDirection::Right, gap, 1.0).unwrap();

        assert_eq!(first.len(), second.len());
        for (first_id, first_pos) in &first {
            let second_pos = second
                .iter()
                .find(|(second_id, _)| second_id == first_id)
                .map(|(_, pos)| *pos)
                .unwrap();
            assert!((first_pos.x - second_pos.x).abs() < 0.001);
            assert!((first_pos.y - second_pos.y).abs() < 0.001);
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
            assert!(
                (first_pos.x - second_pos.x).abs() < 0.001,
                "item {first_id} x moved from {} to {}",
                first_pos.x,
                second_pos.x
            );
            assert!(
                (first_pos.y - second_pos.y).abs() < 0.001,
                "item {first_id} y moved from {} to {}",
                first_pos.y,
                second_pos.y
            );
        }
    }

    #[test]
    fn compact_layout_for_sizes_preserves_input_order() {
        let sizes = [
            eframe::egui::vec2(90.0, 40.0),
            eframe::egui::vec2(300.0, 520.0),
            eframe::egui::vec2(140.0, 180.0),
        ];

        let placeholders = compact_layout_for_sizes(&sizes, 12.0, eframe::egui::pos2(100.0, 200.0));

        assert_eq!(placeholders.len(), sizes.len());
        for (index, (_, size)) in placeholders.iter().enumerate() {
            assert_eq!(*size, sizes[index]);
        }
    }

    #[test]
    fn balanced_grid_layout_avoids_single_column_for_equal_approximate_batch() {
        let sizes = vec![eframe::egui::vec2(360.0, 240.0); 13];
        let placeholders =
            balanced_grid_layout_for_sizes(&sizes, 24.0, eframe::egui::pos2(100.0, 200.0));
        let distinct_x = placeholders
            .iter()
            .fold(Vec::<i32>::new(), |mut values, (center, _)| {
                let rounded = center.x.round() as i32;
                if !values.contains(&rounded) {
                    values.push(rounded);
                }
                values
            });
        let distinct_y = placeholders
            .iter()
            .fold(Vec::<i32>::new(), |mut values, (center, _)| {
                let rounded = center.y.round() as i32;
                if !values.contains(&rounded) {
                    values.push(rounded);
                }
                values
            });

        assert_eq!(placeholders.len(), sizes.len());
        assert!(distinct_x.len() > 1);
        assert!(distinct_y.len() > 1);
    }

    #[test]
    fn compact_layout_keeps_hole_prone_fixture_dense() {
        let items = hole_prone_items();
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let item_area = items
            .iter()
            .map(|item| item.width * item.height)
            .sum::<f32>();
        let occupancy = item_area / (compact_width * compact_height);

        assert!(
            occupancy > 0.75,
            "compact arrangement should keep the mixed-size fixture dense"
        );
    }

    #[test]
    fn compact_layout_prefers_dense_candidate_over_square_with_holes() {
        let items = vec![
            layout_item(1, 300.0, 300.0),
            layout_item(2, 40.0, 50.0),
            layout_item(3, 50.0, 90.0),
            layout_item(4, 300.0, 60.0),
        ];
        let gap = 12.0;

        let (_, compact_width, compact_height) = best_compact_layout(&items, gap).unwrap();
        let item_area = items
            .iter()
            .map(|item| (item.width + gap) * (item.height + gap))
            .sum::<f32>();
        let occupancy = item_area / (compact_width * compact_height);

        assert!(
            occupancy > 0.75,
            "compact arrangement should prefer fewer holes over a more square empty box"
        );
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
    fn compact_arrangement_respects_rotated_placement_footprints() {
        let mut state = CanvasState::default();
        let gap = 12.0;

        eframe::egui::__run_test_ui(|ui| {
            push_test_image(
                ui,
                &mut state,
                1,
                100,
                300,
                1.0,
                eframe::egui::pos2(0.0, 0.0),
            );
            push_test_image(
                ui,
                &mut state,
                2,
                180,
                100,
                1.0,
                eframe::egui::pos2(500.0, 0.0),
            );
        });
        assert!(state.set_image_rotation(1, std::f32::consts::FRAC_PI_2));
        state.select_placements(&[1, 2]);

        assert_eq!(state.arrange_selected_compact(gap), 2);

        let first = state
            .document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == 1)
            .unwrap();
        let second = state
            .document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == 2)
            .unwrap();
        let first_rect = placement_visual_bounds_world(first);
        let second_rect = placement_visual_bounds_world(second);

        assert!(
            rectangles_respect_gap(first_rect, second_rect, gap),
            "rotated placement footprint should not overlap arranged neighbor"
        );
    }

    #[test]
    fn compact_arrangement_normalizes_one_world_unit_width_difference() {
        let mut state = CanvasState::default();
        let gap = 12.0;

        eframe::egui::__run_test_ui(|ui| {
            push_test_image(
                ui,
                &mut state,
                1,
                100,
                100,
                1.0,
                eframe::egui::pos2(0.0, 0.0),
            );
            push_test_image(
                ui,
                &mut state,
                2,
                101,
                100,
                1.0,
                eframe::egui::pos2(130.0, 0.0),
            );
        });
        state.select_placements(&[1, 2]);

        assert_eq!(state.arrange_selected_compact(gap), 2);

        let first = state
            .document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == 1)
            .unwrap();
        let second = state
            .document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == 2)
            .unwrap();
        let first_width = first
            .placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .width as f32
            * first.placement.state.scale;
        let second_width = second
            .placement
            .asset
            .as_ref()
            .expect("loaded placement asset")
            .width as f32
            * second.placement.state.scale;
        assert!((first_width - second_width).abs() < 0.001);
    }

    #[test]
    fn compact_arrangement_rejects_width_normalization_that_disrupts_height() {
        let mut state = CanvasState::default();
        let gap = 12.0;

        eframe::egui::__run_test_ui(|ui| {
            push_test_image(
                ui,
                &mut state,
                1,
                10,
                1000,
                1.0,
                eframe::egui::pos2(0.0, 0.0),
            );
            push_test_image(
                ui,
                &mut state,
                2,
                11,
                1000,
                1.0,
                eframe::egui::pos2(40.0, 0.0),
            );
        });
        state.select_placements(&[1, 2]);

        assert_eq!(state.arrange_selected_compact(gap), 2);

        let first = state
            .document
            .rendered_placements
            .iter()
            .find(|image| image.placement.state.id == 1)
            .unwrap();
        assert!((first.placement.state.scale - 1.0).abs() < 0.001);
    }

    #[test]
    fn compact_preview_layout_does_not_normalize_sizes() {
        let sizes = [
            eframe::egui::vec2(100.0, 100.0),
            eframe::egui::vec2(101.0, 100.0),
        ];

        let placeholders = compact_layout_for_sizes(&sizes, 12.0, eframe::egui::pos2(0.0, 0.0));

        assert_eq!(placeholders[0].1, sizes[0]);
        assert_eq!(placeholders[1].1, sizes[1]);
    }

    #[test]
    fn compact_arrangement_is_idempotent_for_same_selection() {
        let mut state = CanvasState::default();
        let gap = 12.0;

        eframe::egui::__run_test_ui(|ui| {
            push_test_image(
                ui,
                &mut state,
                1,
                180,
                120,
                1.0,
                eframe::egui::pos2(-300.0, 0.0),
            );
            push_test_image(
                ui,
                &mut state,
                2,
                90,
                180,
                1.0,
                eframe::egui::pos2(0.0, 200.0),
            );
            push_test_image(
                ui,
                &mut state,
                3,
                140,
                90,
                1.0,
                eframe::egui::pos2(240.0, -120.0),
            );
            push_test_image(
                ui,
                &mut state,
                4,
                80,
                80,
                1.0,
                eframe::egui::pos2(460.0, 80.0),
            );
        });
        state.select_placements(&[1, 2, 3, 4]);

        assert_eq!(state.arrange_selected_compact(gap), 4);
        let first_positions = selected_image_positions(&state);
        assert_eq!(state.arrange_selected_compact(gap), 0);
        let second_positions = selected_image_positions(&state);

        assert_eq!(first_positions.len(), second_positions.len());
        for ((first_id, first_pos), (second_id, second_pos)) in
            first_positions.iter().zip(second_positions.iter())
        {
            assert_eq!(first_id, second_id);
            assert!(
                (first_pos.x - second_pos.x).abs() < 0.001,
                "item {first_id} x moved from {} to {}",
                first_pos.x,
                second_pos.x
            );
            assert!(
                (first_pos.y - second_pos.y).abs() < 0.001,
                "item {first_id} y moved from {} to {}",
                first_pos.y,
                second_pos.y
            );
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
        state.select_placements(&[1]);

        open_context_menu_at_pointer(
            &mut state,
            rect,
            rect.center() + eframe::egui::vec2(100.0, 0.0),
        );

        assert_eq!(state.selected_placement_ids(), vec![2]);
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
        state.select_placements(&[1]);

        open_context_menu_at_pointer(
            &mut state,
            rect,
            rect.center() + eframe::egui::vec2(120.0, 20.0),
        );

        assert!(state.selected_placement_ids().is_empty());
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

        assert_eq!(state.selected_placement_ids(), vec![1]);
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
    fn selected_canvas_item_moves_to_absolute_front_and_back() {
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
        state.select_content(&[1], &[]);

        assert!(state.move_selected_canvas_items_to_front());
        assert_eq!(
            state.ordered_canvas_items_back_to_front(),
            vec![
                CanvasStackItem::Annotation(20),
                CanvasStackItem::Placement(2),
                CanvasStackItem::Placement(1),
            ]
        );

        assert!(state.move_selected_canvas_items_to_back());
        assert_eq!(
            state.ordered_canvas_items_back_to_front(),
            vec![
                CanvasStackItem::Placement(1),
                CanvasStackItem::Annotation(20),
                CanvasStackItem::Placement(2),
            ]
        );
    }

    #[test]
    fn mixed_selected_canvas_items_move_as_ordered_block() {
        let mut state = CanvasState::default();
        eframe::egui::__run_test_ui(|ui| {
            push_test_image(ui, &mut state, 1, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
            push_test_image(ui, &mut state, 2, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
            push_test_image(ui, &mut state, 3, 80, 80, 1.0, eframe::egui::pos2(0.0, 0.0));
        });
        for annotation_id in [20, 21] {
            state.add_annotation(CanvasAnnotation {
                id: annotation_id,
                kind: CanvasAnnotationKind::Textbox,
                start_x: -40.0,
                start_y: -40.0,
                end_x: 40.0,
                end_y: 40.0,
                text: format!("note {annotation_id}"),
                text_size: default_text_size(),
                stroke_width: default_stroke_width(),
                color: default_annotation_color(),
                background_enabled: false,
                archived: false,
            });
        }
        state.set_canvas_stack_order(vec![
            CanvasStackItem::Placement(1),
            CanvasStackItem::Annotation(20),
            CanvasStackItem::Placement(2),
            CanvasStackItem::Annotation(21),
            CanvasStackItem::Placement(3),
        ]);
        state.select_content(&[3, 1], &[20]);

        assert!(state.move_selected_canvas_items_to_front());
        assert_eq!(
            state.ordered_canvas_items_back_to_front(),
            vec![
                CanvasStackItem::Placement(2),
                CanvasStackItem::Annotation(21),
                CanvasStackItem::Placement(1),
                CanvasStackItem::Annotation(20),
                CanvasStackItem::Placement(3),
            ]
        );

        assert!(state.move_selected_canvas_items_to_back());
        assert_eq!(
            state.ordered_canvas_items_back_to_front(),
            vec![
                CanvasStackItem::Placement(1),
                CanvasStackItem::Annotation(20),
                CanvasStackItem::Placement(3),
                CanvasStackItem::Placement(2),
                CanvasStackItem::Annotation(21),
            ]
        );
    }

    #[test]
    fn moved_canvas_stack_order_controls_overlapping_context_selection() {
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
        state.select_content(&[], &[20]);

        assert!(state.move_selected_canvas_items_to_front());
        open_context_menu_at_pointer(&mut state, rect, rect.center());

        assert!(state.selected_placement_ids().is_empty());
        assert_eq!(state.selected_annotation_ids(), vec![20]);
        assert!(state.take_context_menu_request().is_some());
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

        assert_eq!(state.selected_placement_ids(), vec![1]);
        assert_eq!(state.selected_annotation_ids(), vec![20]);
        assert!(state.take_context_menu_request().is_some());
    }

    fn layout_item(id: i64, width: f32, height: f32) -> SelectedLayoutItem {
        positioned_layout_item(id, 0.0, 0.0, width, height)
    }

    fn positioned_layout_item(
        id: i64,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> SelectedLayoutItem {
        SelectedLayoutItem {
            id,
            x,
            y,
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
        placement_id: i64,
    ) -> eframe::egui::Rect {
        let item = items.iter().find(|item| item.id == placement_id).unwrap();
        let center = placements
            .iter()
            .find(|(candidate_placement_id, _)| *candidate_placement_id == placement_id)
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

    fn placement_visual_bounds_world(image: &RenderedPlacement) -> eframe::egui::Rect {
        let visual_size = rotated_visual_size(
            image
                .placement
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .width as f32
                * image.placement.state.scale.max(0.01),
            image
                .placement
                .asset
                .as_ref()
                .expect("loaded placement asset")
                .height as f32
                * image.placement.state.scale.max(0.01),
            image.placement.state.rotation,
        );
        eframe::egui::Rect::from_center_size(
            eframe::egui::pos2(
                image.placement.state.center.x,
                image.placement.state.center.y,
            ),
            visual_size,
        )
    }

    fn selected_image_positions(state: &CanvasState) -> Vec<(i64, eframe::egui::Pos2)> {
        let mut positions = state
            .document
            .rendered_placements
            .iter()
            .filter(|image| state.is_selected(image.placement.state.id))
            .map(|image| {
                (
                    image.placement.state.id,
                    eframe::egui::pos2(
                        image.placement.state.center.x,
                        image.placement.state.center.y,
                    ),
                )
            })
            .collect::<Vec<_>>();
        positions.sort_by_key(|(placement_id, _)| *placement_id);
        positions
    }
}
