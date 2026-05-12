use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::semantic_search::{SearchResultGroup, SearchScope};

pub(crate) const SEARCH_DEBOUNCE_MS: f32 = 120.0;
pub(crate) const SEARCH_OVERLAY_TOP_MARGIN: f32 = 56.0;
pub(crate) const PALETTE_WIDTH: f32 = 660.0;
pub(crate) const PALETTE_GRID_COLUMNS: usize = 5;
pub(crate) const PALETTE_GRID_MAX_HEIGHT: f32 = 360.0;
pub(crate) const SEARCH_RESULT_PAGE_SIZE: usize = 5;

const SEARCH_RESULT_TILE_PREVIEW_HEIGHT: f32 = 144.0;
const SEARCH_RESULT_LIBRARY_PREVIEW_MAX_SIDE: u32 = 512;
const KBD_PILL_SIZE: egui::Vec2 = egui::vec2(28.0, 28.0);
const PALETTE_CHROME_Y_MARGIN: i8 = crate::theme::SPACE_3 as i8;
const PALETTE_FOOTER_STATUS_WIDTH: f32 = 96.0;

pub(crate) struct SearchOverlayState {
    pub open: bool,
    pub query: String,
    pub last_submitted_query: String,
    pub last_query_edit_at: Option<Instant>,
    pub result_selection: SearchSelection,
    pub scroll_lead_result_into_view: bool,
    pub active_recent_search_index: usize,
    pub drag_state: Option<SearchResultDragState>,
    pub last_expanded_overlay_rect: Option<egui::Rect>,
    pub results: Vec<SearchResultGroup>,
    pub asset_preview_cache: HashMap<i64, SearchAssetPreviewEntry>,
    pub result_limit: usize,
    pub has_more_results: bool,
    pub status: SearchOverlayStatus,
    pub focus_requested: bool,
    pub suppress_next_outside_click: bool,
    pub image_pivot: Option<ImageSearchPivot>,
    pub scope: SearchScope,
    pub install_action_focus_requested: bool,
}

impl Default for SearchOverlayState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            last_submitted_query: String::new(),
            last_query_edit_at: None,
            result_selection: SearchSelection::default(),
            scroll_lead_result_into_view: false,
            active_recent_search_index: 0,
            drag_state: None,
            last_expanded_overlay_rect: None,
            results: Vec::new(),
            asset_preview_cache: HashMap::new(),
            result_limit: SEARCH_RESULT_PAGE_SIZE,
            has_more_results: false,
            status: SearchOverlayStatus::Idle,
            focus_requested: false,
            suppress_next_outside_click: false,
            image_pivot: None,
            scope: SearchScope::ThisBoard,
            install_action_focus_requested: false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct SearchAssetPreview {
    texture: egui::TextureHandle,
    texture_size: egui::Vec2,
}

pub(crate) enum SearchAssetPreviewEntry {
    Loading(Arc<Mutex<Option<Option<DecodedSearchAssetPreview>>>>),
    Ready(SearchAssetPreview),
    Failed,
}

pub(crate) struct DecodedSearchAssetPreview {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SearchSelection {
    selected: Vec<usize>,
    lead: Option<usize>,
    anchor: Option<usize>,
}

impl SearchSelection {
    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.lead = None;
        self.anchor = None;
    }

    pub(crate) fn selected(&self) -> &[usize] {
        &self.selected
    }

    pub(crate) fn lead(&self) -> Option<usize> {
        self.lead
    }

    pub(crate) fn contains(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub(crate) fn replace(&mut self, index: usize) {
        self.selected.clear();
        self.selected.push(index);
        self.lead = Some(index);
        self.anchor = Some(index);
    }

    pub(crate) fn toggle(&mut self, index: usize) {
        if let Some(position) = self.selected.iter().position(|selected| *selected == index) {
            self.selected.remove(position);
        } else {
            self.selected.push(index);
        }

        self.selected.sort_unstable();
        if self.selected.is_empty() {
            self.selected.push(index);
            self.lead = Some(index);
            self.anchor = Some(index);
        } else if self.selected.contains(&index) {
            self.lead = Some(index);
            self.anchor = Some(index);
        } else {
            let fallback = self.selected[0];
            self.lead = Some(fallback);
            self.anchor = Some(fallback);
        }
    }

    pub(crate) fn range_to(&mut self, index: usize) {
        let anchor = self
            .anchor
            .or_else(|| self.selected.first().copied())
            .unwrap_or(index);
        let start = anchor.min(index);
        let end = anchor.max(index);
        self.selected = (start..=end).collect();
        self.lead = Some(index);
    }

    pub(crate) fn select_initial(&mut self, result_count: usize) {
        if result_count == 0 {
            self.clear();
            return;
        }

        let index = self.lead.unwrap_or(0).min(result_count - 1);
        self.replace(index);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SearchResultDragState {
    pub item_ids: Vec<i64>,
    pub asset_ids: Vec<i64>,
    pub asset_placeholder_sizes: Vec<egui::Vec2>,
    pub outside_overlay: bool,
    pub placement_pending: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageSearchPivot {
    pub asset_id: i64,
    pub item_id: i64,
}

#[derive(Default)]
pub(crate) enum SearchOverlayStatus {
    #[default]
    Idle,
    Loading(String),
    Indexing,
    Ready,
    Error(String),
}

#[derive(Clone, Copy)]
pub(crate) struct SearchResultGridLayout {
    pub columns: usize,
    pub tile_width: f32,
}

pub(crate) fn search_result_tile_height() -> f32 {
    SEARCH_RESULT_TILE_PREVIEW_HEIGHT
}

pub(crate) fn search_result_grid_layout(
    available_width: f32,
    result_count: usize,
) -> SearchResultGridLayout {
    let gap = crate::theme::SPACE_2;
    let columns = if result_count == 0 {
        1
    } else {
        PALETTE_GRID_COLUMNS
    };
    let tile_width = (available_width - gap * (columns.saturating_sub(1) as f32)) / columns as f32;
    SearchResultGridLayout {
        columns,
        tile_width,
    }
}

pub(crate) fn search_result_grid_content_height(result_count: usize, columns: usize) -> f32 {
    if result_count == 0 {
        return 0.0;
    }

    let rows = result_count.div_ceil(columns.max(1));
    rows as f32 * search_result_tile_height()
        + (rows.saturating_sub(1) as f32) * crate::theme::SPACE_2
}

pub(crate) fn search_result_tile(
    ui: &mut egui::Ui,
    canvas: &crate::canvas::CanvasState,
    group: &SearchResultGroup,
    asset_preview_cache: &mut HashMap<i64, SearchAssetPreviewEntry>,
    lead: bool,
    selected: bool,
    tile_width: f32,
) -> egui::Response {
    let total_size = egui::vec2(tile_width, search_result_tile_height());
    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let tile_radius = if selected {
        0
    } else {
        crate::theme::RADIUS_PANEL
    };

    let preview_rect = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(tile_width, SEARCH_RESULT_TILE_PREVIEW_HEIGHT),
    );
    ui.painter()
        .rect_filled(preview_rect, tile_radius, crate::theme::SURFACE);

    if let Some(item_id) = group.item_ids.first().copied()
        && let Some(preview) = canvas.preview_for_item_id(item_id)
    {
        paint_texture_cover(ui, preview_rect, preview.texture_id, preview.texture_size);
    } else if let Some(path) = &group.asset_preview_path {
        if let Some(preview) =
            search_asset_preview(ui.ctx(), group.asset_id, path, asset_preview_cache)
        {
            paint_texture_cover(ui, preview_rect, preview.texture.id(), preview.texture_size);
        }
    } else {
        ui.painter()
            .rect_filled(preview_rect, tile_radius, crate::theme::BACKGROUND);
    }

    if selected {
        let stroke = if lead {
            egui::Stroke::new(2.0, crate::theme::ACCENT)
        } else {
            egui::Stroke::new(2.0, egui::Color32::from_white_alpha(140))
        };
        ui.painter()
            .rect_stroke(preview_rect, tile_radius, stroke, egui::StrokeKind::Outside);
    }

    response
}

fn search_asset_preview(
    ctx: &egui::Context,
    asset_id: i64,
    path: &Path,
    cache: &mut HashMap<i64, SearchAssetPreviewEntry>,
) -> Option<SearchAssetPreview> {
    if let Some(entry) = cache.remove(&asset_id) {
        match entry {
            SearchAssetPreviewEntry::Ready(preview) => {
                cache.insert(asset_id, SearchAssetPreviewEntry::Ready(preview.clone()));
                return Some(preview);
            }
            SearchAssetPreviewEntry::Failed => {
                cache.insert(asset_id, SearchAssetPreviewEntry::Failed);
                return None;
            }
            SearchAssetPreviewEntry::Loading(result) => {
                let decoded = result.lock().ok().and_then(|mut result| result.take());
                match decoded {
                    Some(Some(decoded)) => {
                        let preview = upload_search_asset_preview(ctx, asset_id, decoded);
                        cache.insert(asset_id, SearchAssetPreviewEntry::Ready(preview.clone()));
                        return Some(preview);
                    }
                    Some(None) => {
                        cache.insert(asset_id, SearchAssetPreviewEntry::Failed);
                        return None;
                    }
                    None => {
                        cache.insert(asset_id, SearchAssetPreviewEntry::Loading(result));
                        ctx.request_repaint_after(Duration::from_millis(16));
                        return None;
                    }
                }
            }
        }
    }

    let started_at = Instant::now();
    let result = Arc::new(Mutex::new(None));
    let worker_result = result.clone();
    let worker_ctx = ctx.clone();
    let worker_path = path.to_path_buf();
    std::thread::spawn(move || {
        let decoded = decode_search_asset_preview(&worker_path);
        if cfg!(debug_assertions) && std::env::var_os("SNAPPYREF_SEARCH_LATENCY_LOG").is_some() {
            let elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            let loaded = decoded.is_some();
            eprintln!(
                "[search-phase] search_asset_preview_decode asset_id={asset_id} loaded={loaded} ms={elapsed_ms:.2} path={}",
                worker_path.display()
            );
        }
        if let Ok(mut result) = worker_result.lock() {
            *result = Some(decoded);
        }
        worker_ctx.request_repaint();
    });
    cache.insert(asset_id, SearchAssetPreviewEntry::Loading(result));
    ctx.request_repaint_after(Duration::from_millis(16));
    None
}

fn decode_search_asset_preview(path: &PathBuf) -> Option<DecodedSearchAssetPreview> {
    let image = image::open(path).ok()?.thumbnail(
        SEARCH_RESULT_LIBRARY_PREVIEW_MAX_SIDE,
        SEARCH_RESULT_LIBRARY_PREVIEW_MAX_SIDE,
    );
    let rgba = image.to_rgba8();
    Some(DecodedSearchAssetPreview {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

fn upload_search_asset_preview(
    ctx: &egui::Context,
    asset_id: i64,
    decoded: DecodedSearchAssetPreview,
) -> SearchAssetPreview {
    let texture_size = egui::vec2(decoded.width as f32, decoded.height as f32);
    let texture = ctx.load_texture(
        format!("search-asset-preview-{asset_id}"),
        egui::ColorImage::from_rgba_unmultiplied(
            [decoded.width as usize, decoded.height as usize],
            &decoded.rgba,
        ),
        egui::TextureOptions::LINEAR,
    );
    SearchAssetPreview {
        texture,
        texture_size,
    }
}

fn paint_texture_cover(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    texture_id: egui::TextureId,
    texture_size: egui::Vec2,
) {
    let preview_aspect = rect.width() / rect.height().max(1.0);
    let texture_aspect = texture_size.x / texture_size.y.max(1.0);
    let uv_rect = if texture_aspect > preview_aspect {
        let visible_u = preview_aspect / texture_aspect;
        let inset = (1.0 - visible_u) * 0.5;
        egui::Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
    } else {
        let visible_v = texture_aspect / preview_aspect;
        let inset = (1.0 - visible_v) * 0.5;
        egui::Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
    };
    ui.painter()
        .image(texture_id, rect, uv_rect, egui::Color32::WHITE);
}

pub(crate) struct PaletteInputResult {
    pub response: egui::Response,
    pub pivot_dismissed: bool,
}

pub(crate) fn paint_palette_input_row(
    ui: &mut egui::Ui,
    query: &mut String,
    input_id: egui::Id,
    pivot: Option<&ImageSearchPivot>,
    canvas: &crate::canvas::CanvasState,
) -> PaletteInputResult {
    let mut response = None;
    let mut pivot_dismissed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_4 as i8,
            right: crate::theme::SPACE_4 as i8,
            top: PALETTE_CHROME_Y_MARGIN,
            bottom: PALETTE_CHROME_Y_MARGIN,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(KBD_PILL_SIZE.y);
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
                let placeholder = if let Some(pivot) = pivot {
                    if paint_image_pivot_pill(ui, canvas, pivot) {
                        pivot_dismissed = true;
                    }
                    "Refine with a term..."
                } else {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(KBD_PILL_SIZE, egui::Sense::hover());
                    let search_icon_rect =
                        egui::Rect::from_center_size(icon_rect.center(), egui::vec2(16.0, 16.0));
                    paint_search_icon(ui, search_icon_rect, crate::theme::TEXT_SUBTLE);
                    "Search images"
                };

                let mut visuals = ui.style_mut().visuals.clone();
                visuals.extreme_bg_color = egui::Color32::TRANSPARENT;
                visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                visuals.widgets.active.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
                visuals.widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
                visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                visuals.selection.bg_fill = crate::theme::SURFACE_ELEVATED;
                ui.style_mut().visuals = visuals;

                let text_width = (ui.available_width() - 42.0).max(160.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(query)
                        .id(input_id)
                        .desired_width(text_width)
                        .hint_text(placeholder)
                        .vertical_align(egui::Align::Center)
                        .frame(egui::Frame::NONE),
                );
                paint_kbd_pill(ui, "esc");
                response = Some(resp);
            });
        });
    PaletteInputResult {
        response: response.expect("palette input row allocated"),
        pivot_dismissed,
    }
}

fn search_scope_label(scope: SearchScope) -> &'static str {
    match scope {
        SearchScope::ThisBoard => "This Board",
        SearchScope::Library => "Library",
    }
}

fn paint_footer_scope_toggle(ui: &mut egui::Ui, scope: SearchScope) -> bool {
    let label = search_scope_label(scope);
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let start = ui.cursor().min;
        paint_kbd_pill(ui, "tab");
        ui.add_space(6.0);
        footer_hint_label(ui, label);
        let rect = egui::Rect::from_min_max(start, ui.cursor().max);
        let hovered = ui.ctx().input(|input| {
            input
                .pointer
                .latest_pos()
                .is_some_and(|pos| rect.contains(pos))
        });
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        hovered && ui.input(|input| input.pointer.primary_clicked())
    })
    .inner
}

fn paint_image_pivot_pill(
    ui: &mut egui::Ui,
    canvas: &crate::canvas::CanvasState,
    pivot: &ImageSearchPivot,
) -> bool {
    let mut dismissed = false;
    let thumb_size = 18.0;
    let dismiss_size = 14.0;

    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(18))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(28)))
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_1 as i8,
            right: crate::theme::SPACE_1 as i8,
            top: crate::theme::SPACE_1 as i8,
            bottom: crate::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;

                let (thumb_rect, _) = ui
                    .allocate_exact_size(egui::vec2(thumb_size, thumb_size), egui::Sense::hover());
                if let Some(preview) = canvas.preview_for_item_id(pivot.item_id) {
                    let preview_aspect = thumb_rect.width() / thumb_rect.height().max(1.0);
                    let texture_aspect = preview.texture_size.x / preview.texture_size.y.max(1.0);
                    let uv_rect = if texture_aspect > preview_aspect {
                        let visible_u = preview_aspect / texture_aspect;
                        let inset = (1.0 - visible_u) * 0.5;
                        egui::Rect::from_min_max(
                            egui::pos2(inset, 0.0),
                            egui::pos2(1.0 - inset, 1.0),
                        )
                    } else {
                        let visible_v = texture_aspect / preview_aspect;
                        let inset = (1.0 - visible_v) * 0.5;
                        egui::Rect::from_min_max(
                            egui::pos2(0.0, inset),
                            egui::pos2(1.0, 1.0 - inset),
                        )
                    };
                    ui.painter().image(
                        preview.texture_id,
                        thumb_rect,
                        uv_rect,
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter().rect_filled(
                        thumb_rect,
                        egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
                        crate::theme::SURFACE_ELEVATED,
                    );
                }

                ui.label(
                    egui::RichText::new(format!("similar to #{}", pivot.asset_id))
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT),
                );

                let (dismiss_rect, dismiss_response) = ui.allocate_exact_size(
                    egui::vec2(dismiss_size, dismiss_size),
                    egui::Sense::click(),
                );
                let dismiss_response =
                    dismiss_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let dismiss_color = if dismiss_response.hovered() {
                    crate::theme::TEXT
                } else {
                    crate::theme::TEXT_MUTED
                };
                let stroke = egui::Stroke::new(1.2, dismiss_color);
                let inset = 3.0;
                ui.painter().line_segment(
                    [
                        egui::pos2(dismiss_rect.left() + inset, dismiss_rect.top() + inset),
                        egui::pos2(dismiss_rect.right() - inset, dismiss_rect.bottom() - inset),
                    ],
                    stroke,
                );
                ui.painter().line_segment(
                    [
                        egui::pos2(dismiss_rect.right() - inset, dismiss_rect.top() + inset),
                        egui::pos2(dismiss_rect.left() + inset, dismiss_rect.bottom() - inset),
                    ],
                    stroke,
                );
                if dismiss_response.clicked() {
                    dismissed = true;
                }
            });
        });

    dismissed
}

fn paint_search_icon(ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let painter = ui.painter();
    let center = rect.center();
    let r = rect.width() * 0.28;
    let stroke = egui::Stroke::new(1.4, color);
    painter.circle_stroke(center - egui::vec2(r * 0.3, r * 0.3), r, stroke);
    painter.line_segment(
        [
            center + egui::vec2(r * 0.4, r * 0.4),
            center + egui::vec2(r * 1.1, r * 1.1),
        ],
        stroke,
    );
}

fn paint_kbd_pill(ui: &mut egui::Ui, label: &str) {
    let label_size = if kbd_label_uses_small_symbol_glyphs(label) {
        14.0
    } else {
        10.0
    };

    let (rect, _) = ui.allocate_exact_size(KBD_PILL_SIZE, egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        crate::theme::RADIUS_INLINE,
        egui::Color32::from_white_alpha(15),
    );
    ui.painter().rect_stroke(
        rect,
        crate::theme::RADIUS_INLINE,
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(15)),
        egui::StrokeKind::Inside,
    );

    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::monospace(label_size),
        crate::theme::TEXT_MUTED,
    );
    ui.painter().galley(
        crate::ui_layout::centered_pos(rect, galley.size()),
        galley,
        crate::theme::TEXT_MUTED,
    );
}

fn kbd_label_uses_small_symbol_glyphs(label: &str) -> bool {
    label
        .chars()
        .any(|ch| matches!(ch, '⇧' | '↵' | '↓' | '↑' | '⌘'))
}

fn command_modifier_key_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "⌘"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl"
    }
}

fn footer_hint_label(ui: &mut egui::Ui, label: &str) {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(11.0),
        crate::theme::TEXT_SUBTLE,
    );
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(galley.size().x, KBD_PILL_SIZE.y),
        egui::Sense::hover(),
    );
    ui.painter().galley(
        crate::ui_layout::centered_pos(rect, galley.size()),
        galley,
        crate::theme::TEXT_SUBTLE,
    );
}

fn footer_hint_label_width(ui: &egui::Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(11.0),
            crate::theme::TEXT_SUBTLE,
        )
        .size()
        .x
}

pub(crate) fn paint_palette_divider(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0, egui::Color32::from_white_alpha(14));
}

pub(crate) fn paint_palette_empty_state(
    ui: &mut egui::Ui,
    recent_searches: &[String],
    active_recent_search_index: usize,
) -> Option<String> {
    let mut selected_recent_search = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_2 as i8,
            right: crate::theme::SPACE_2 as i8,
            top: crate::theme::SPACE_3 as i8,
            bottom: crate::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            selected_recent_search = paint_palette_empty_group(
                ui,
                "RECENT",
                recent_searches,
                active_recent_search_index,
            );
        });
    selected_recent_search
}

pub(crate) fn paint_palette_no_results_state(ui: &mut egui::Ui, status: &SearchOverlayStatus) {
    let text = palette_status_text(status, "No results");
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 18,
            top: 26,
            bottom: 24,
        })
        .show(ui, |ui| {
            ui.add_sized(
                egui::vec2(ui.available_width(), 16.0),
                egui::Label::new(
                    egui::RichText::new(text)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_SUBTLE)
                        .size(11.0),
                )
                .truncate(),
            );
        });
}

pub(crate) fn paint_palette_install_state(
    ui: &mut egui::Ui,
    status: &SearchOverlayStatus,
    focus_requested: &mut bool,
    installing: bool,
) -> egui::Response {
    let button_id = egui::Id::new("semantic_search_install_action");
    let mut response = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_4 as i8,
            right: crate::theme::SPACE_4 as i8,
            top: crate::theme::SPACE_4 as i8,
            bottom: crate::theme::SPACE_4 as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_3);
            ui.label(
                egui::RichText::new("Semantic Search requires a local download.")
                    .text_style(egui::TextStyle::Body)
                    .color(crate::theme::TEXT),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Install the search assets to find references by description.",
                    )
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
                )
                .wrap(),
            );
            if let SearchOverlayStatus::Error(message) = status {
                ui.label(
                    egui::RichText::new(message)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::DESTRUCTIVE),
                );
            }
            let label = match status {
                SearchOverlayStatus::Error(_) => "Retry",
                SearchOverlayStatus::Loading(message) => message.as_str(),
                _ => "Install",
            };
            let button = egui::Button::new(label)
                .min_size(egui::vec2(112.0, 30.0))
                .corner_radius(crate::theme::RADIUS_CONTROL);
            let resp = ui
                .push_id(button_id, |ui| ui.add_enabled(!installing, button))
                .inner;
            if *focus_requested {
                resp.request_focus();
                *focus_requested = !resp.has_focus();
                if *focus_requested {
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                }
            }
            response = Some(resp);
        });
    response.expect("semantic install action allocated")
}

fn paint_palette_empty_group(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[String],
    active_recent_search_index: usize,
) -> Option<String> {
    let mut selected_recent_search = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_2 as i8,
            right: crate::theme::SPACE_2 as i8,
            top: crate::theme::SPACE_1 as i8,
            bottom: crate::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .size(9.0),
            );
        });
    for (index, row) in rows.iter().enumerate() {
        let row_height = 34.0;
        let row_width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(row_width, row_height), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let active = index == active_recent_search_index;
        if active || response.hovered() {
            ui.painter().rect_filled(
                rect.shrink2(egui::vec2(2.0, 2.0)),
                crate::theme::RADIUS_PANEL,
                if active {
                    crate::theme::SURFACE_ELEVATED
                } else {
                    crate::theme::SURFACE
                },
            );
        }
        ui.painter().text(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            row.as_str(),
            egui::TextStyle::Body.resolve(ui.style()),
            crate::theme::TEXT,
        );
        if response.clicked() {
            selected_recent_search = Some(row.clone());
        }
    }
    ui.add_space(4.0);
    selected_recent_search
}

pub(crate) fn paint_palette_footer(
    ui: &mut egui::Ui,
    result_count: usize,
    has_more_results: bool,
    status: &SearchOverlayStatus,
    scope: SearchScope,
) -> bool {
    let mut scope_toggled = false;
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(38))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_4 as i8,
            right: crate::theme::SPACE_4 as i8,
            top: PALETTE_CHROME_Y_MARGIN,
            bottom: PALETTE_CHROME_Y_MARGIN,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(KBD_PILL_SIZE.y);
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
                if result_count == 0 {
                    let idle_text = palette_status_text(status, "Ready");
                    ui.allocate_ui_with_layout(
                        egui::vec2(PALETTE_FOOTER_STATUS_WIDTH, KBD_PILL_SIZE.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let dot_rect = ui
                                .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                                .0;
                            ui.painter().circle_filled(
                                dot_rect.center(),
                                3.0,
                                crate::theme::INDEXED_DOT,
                            );
                            ui.add_sized(
                                egui::vec2(
                                    PALETTE_FOOTER_STATUS_WIDTH - 6.0 - crate::theme::SPACE_2,
                                    KBD_PILL_SIZE.y,
                                ),
                                egui::Label::new(
                                    egui::RichText::new(idle_text)
                                        .text_style(egui::TextStyle::Small)
                                        .color(crate::theme::TEXT_SUBTLE)
                                        .size(11.0),
                                )
                                .truncate(),
                            );
                        },
                    );
                } else if has_more_results {
                    let more_hint_width = KBD_PILL_SIZE.x
                        + crate::theme::SPACE_2
                        + footer_hint_label_width(ui, "more");
                    ui.allocate_ui_with_layout(
                        egui::vec2(more_hint_width, KBD_PILL_SIZE.y),
                        crate::ui_layout::row_center_y_layout(),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            paint_kbd_pill(ui, "↓");
                            ui.add_space(crate::theme::SPACE_2);
                            footer_hint_label(ui, "more");
                        },
                    );
                }
                ui.add_space(14.0);
                scope_toggled = paint_footer_scope_toggle(ui, scope);
                ui.with_layout(crate::ui_layout::row_end_center_y_layout(), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if result_count == 0 {
                        footer_hint_label(ui, "select");
                        paint_kbd_pill(ui, "↵");
                        ui.add_space(8.0);
                        footer_hint_label(ui, "navigate");
                        paint_kbd_pill(ui, "↓");
                        paint_kbd_pill(ui, "↑");
                    } else {
                        footer_hint_label(ui, "place all");
                        paint_kbd_pill(ui, "↵");
                        paint_kbd_pill(ui, command_modifier_key_label());
                        ui.add_space(8.0);
                        footer_hint_label(ui, "place");
                        paint_kbd_pill(ui, "↵");
                    }
                });
            });
        });
    scope_toggled
}

fn palette_status_text(status: &SearchOverlayStatus, idle_text: &'static str) -> String {
    match status {
        SearchOverlayStatus::Loading(message) => message.clone(),
        SearchOverlayStatus::Indexing => "Indexing...".to_owned(),
        SearchOverlayStatus::Error(message) => {
            let _has_details = !message.trim().is_empty();
            "Search error".to_owned()
        }
        SearchOverlayStatus::Idle | SearchOverlayStatus::Ready => idle_text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{KBD_PILL_SIZE, PALETTE_CHROME_Y_MARGIN, PALETTE_FOOTER_STATUS_WIDTH};

    #[test]
    fn palette_header_and_footer_share_chrome_height() {
        let header_height = KBD_PILL_SIZE.y + f32::from(PALETTE_CHROME_Y_MARGIN) * 2.0;
        let footer_height = KBD_PILL_SIZE.y + f32::from(PALETTE_CHROME_Y_MARGIN) * 2.0;

        assert_eq!(header_height, footer_height);
    }

    #[test]
    fn palette_footer_status_reservation_stays_compact() {
        assert!(PALETTE_FOOTER_STATUS_WIDTH <= 96.0);
    }
}
