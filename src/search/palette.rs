use super::selection::SearchSelection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::search::semantic::SearchResultAssetEntry;
use crate::ui::shortcut::{self, Key};

// Search palette chrome is an overlay workflow, not a second sidebar.
//
// It uses one fixed panel width, a text-first input row, key chip hints,
// library-wide Asset Result tiles, and compact footer metadata. Selected
// result thumbnails share the same square 1px `TEXT` border language as
// selected canvas Placements and search drag previews; selecting a result
// does not imply an existing canvas Placement is selected.
pub(crate) const SEARCH_DEBOUNCE_MS: f32 = 120.0;
pub(crate) const SEARCH_OVERLAY_TOP_MARGIN: f32 = 56.0;
pub(crate) const PALETTE_WIDTH: f32 = 660.0;
pub(crate) const PALETTE_GRID_COLUMNS: usize = 5;
pub(crate) const PALETTE_GRID_MAX_HEIGHT: f32 = 360.0;
pub(crate) const SEARCH_RESULT_PAGE_SIZE: usize = 5;

const SEARCH_RESULT_TILE_PREVIEW_HEIGHT: f32 = 144.0;
const SEARCH_RESULT_LIBRARY_PREVIEW_MAX_SIDE: u32 = 512;
/// The input row's search icon cell, and the height of the chrome rows.
const CHROME_CELL_SIZE: egui::Vec2 = egui::vec2(28.0, 28.0);
const PALETTE_CHROME_Y_MARGIN: i8 = crate::ui::theme::SPACE_3 as i8;
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
    pub results: Vec<SearchResultAssetEntry>,
    pub asset_preview_cache: HashMap<i64, SearchAssetPreviewEntry>,
    pub result_limit: usize,
    pub has_more_results: bool,
    pub status: SearchOverlayStatus,
    pub focus_requested: bool,
    pub suppress_next_outside_click: bool,
    pub image_pivot: Option<ImageSearchPivot>,
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

#[derive(Clone, Debug)]
pub(crate) struct SearchResultDragState {
    pub asset_ids: Vec<i64>,
    pub outside_overlay: bool,
    pub provisional_batch_active: bool,
    pub auto_fit_applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageSearchPivot {
    pub asset_id: i64,
    pub asset_preview_path: Option<PathBuf>,
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
    let gap = crate::ui::theme::SPACE_2;
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
        + (rows.saturating_sub(1) as f32) * crate::ui::theme::SPACE_2
}

/// Paint one library-wide Asset Result tile.
///
/// Hover, press/drag, and selected acquisition feedback is tactile but bounded
/// to painting: allocated grid size does not change, and the selected image
/// border stays square, 1px, and image-aligned.
pub(crate) fn search_result_tile(
    ui: &mut egui::Ui,
    group: &SearchResultAssetEntry,
    asset_preview_cache: &mut HashMap<i64, SearchAssetPreviewEntry>,
    selected: bool,
    tile_width: f32,
) -> egui::Response {
    let total_size = egui::vec2(tile_width, search_result_tile_height());
    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let tile_radius = if selected {
        crate::ui::theme::RADIUS_NONE
    } else {
        crate::ui::theme::RADIUS_COMPACT
    };
    let tile_id = response.id;
    let hover = crate::ui::motion::hover_amount(ui.ctx(), tile_id, response.hovered());
    let press = crate::ui::motion::press_amount(
        ui.ctx(),
        tile_id,
        response.is_pointer_button_down_on() || response.dragged(),
    );
    let acquire = crate::ui::motion::acquire_amount(ui.ctx(), tile_id, selected);

    let preview_rect = egui::Rect::from_min_size(
        rect.min,
        egui::vec2(tile_width, SEARCH_RESULT_TILE_PREVIEW_HEIGHT),
    );
    let content_rect = crate::ui::motion::pressed_rect(preview_rect, press);
    ui.painter()
        .rect_filled(content_rect, tile_radius, crate::ui::theme::SURFACE);

    if let Some(path) = &group.asset_preview_path {
        if let Some(preview) =
            search_asset_preview(ui.ctx(), group.asset_id, path, asset_preview_cache)
        {
            paint_texture_cover(ui, content_rect, preview.texture.id(), preview.texture_size);
        }
    } else {
        ui.painter()
            .rect_filled(content_rect, tile_radius, crate::ui::theme::BACKGROUND);
    }

    let hover_alpha = (14.0 * hover) as u8;
    if hover_alpha > 0 {
        ui.painter().rect_filled(
            content_rect,
            tile_radius,
            egui::Color32::from_white_alpha(hover_alpha),
        );
    }

    if selected {
        let flash_alpha = (24.0 * (1.0 - acquire).clamp(0.0, 1.0)) as u8;
        if flash_alpha > 0 {
            ui.painter().rect_filled(
                content_rect,
                crate::ui::theme::RADIUS_NONE,
                egui::Color32::from_white_alpha(flash_alpha),
            );
        }
        ui.painter().rect_stroke(
            preview_rect,
            crate::ui::theme::RADIUS_NONE,
            egui::Stroke::new(1.0_f32, crate::ui::theme::TEXT),
            egui::StrokeKind::Outside,
        );
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

    let result = Arc::new(Mutex::new(None));
    let worker_result = result.clone();
    let worker_ctx = ctx.clone();
    let worker_path = path.to_path_buf();
    std::thread::spawn(move || {
        let decoded = decode_search_asset_preview(&worker_path);
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
    asset_preview_cache: &mut HashMap<i64, SearchAssetPreviewEntry>,
) -> PaletteInputResult {
    let mut response = None;
    let mut pivot_dismissed = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::ui::theme::SPACE_4 as i8,
            right: crate::ui::theme::SPACE_4 as i8,
            top: PALETTE_CHROME_Y_MARGIN,
            bottom: PALETTE_CHROME_Y_MARGIN,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(CHROME_CELL_SIZE.y);
                ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
                let placeholder = if let Some(pivot) = pivot {
                    if paint_image_pivot_pill(ui, pivot, asset_preview_cache) {
                        pivot_dismissed = true;
                    }
                    "Refine with a term..."
                } else {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(CHROME_CELL_SIZE, egui::Sense::hover());
                    let search_icon_rect =
                        egui::Rect::from_center_size(icon_rect.center(), egui::vec2(16.0, 16.0));
                    paint_search_icon(ui, search_icon_rect, crate::ui::theme::TEXT_SUBTLE);
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
                visuals.selection.bg_fill = crate::ui::theme::SURFACE_ELEVATED;
                ui.style_mut().visuals = visuals;

                const ESC: &[Key] = &[Key::Named("esc")];
                let esc_width = shortcut::width(ui.painter(), ESC);
                let text_width =
                    (ui.available_width() - crate::ui::theme::SPACE_2 - esc_width).max(160.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(query)
                        .id(input_id)
                        .desired_width(text_width)
                        .hint_text(placeholder)
                        .vertical_align(egui::Align::Center)
                        .frame(egui::Frame::NONE),
                );
                shortcut::show(ui, ESC);
                response = Some(resp);
            });
        });
    PaletteInputResult {
        response: response.expect("palette input row allocated"),
        pivot_dismissed,
    }
}

fn paint_image_pivot_pill(
    ui: &mut egui::Ui,
    pivot: &ImageSearchPivot,
    asset_preview_cache: &mut HashMap<i64, SearchAssetPreviewEntry>,
) -> bool {
    let mut dismissed = false;
    let thumb_size = 18.0;
    let dismiss_size = 14.0;

    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(18))
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_white_alpha(28),
        ))
        .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin {
            left: crate::ui::theme::SPACE_1 as i8,
            right: crate::ui::theme::SPACE_1 as i8,
            top: crate::ui::theme::SPACE_1 as i8,
            bottom: crate::ui::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;

                let (thumb_rect, _) = ui
                    .allocate_exact_size(egui::vec2(thumb_size, thumb_size), egui::Sense::hover());
                if let Some(path) = &pivot.asset_preview_path
                    && let Some(preview) =
                        search_asset_preview(ui.ctx(), pivot.asset_id, path, asset_preview_cache)
                {
                    paint_texture_cover(ui, thumb_rect, preview.texture.id(), preview.texture_size);
                } else {
                    ui.painter().rect_filled(
                        thumb_rect,
                        egui::CornerRadius::same(crate::ui::theme::RADIUS_MICRO),
                        crate::ui::theme::SURFACE_ELEVATED,
                    );
                }

                ui.label(
                    egui::RichText::new(format!("similar to #{}", pivot.asset_id))
                        .text_style(egui::TextStyle::Body)
                        .color(crate::ui::theme::TEXT),
                );

                let (dismiss_rect, dismiss_response) = ui.allocate_exact_size(
                    egui::vec2(dismiss_size, dismiss_size),
                    egui::Sense::click(),
                );
                let dismiss_response =
                    dismiss_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                let dismiss_color = if dismiss_response.hovered() {
                    crate::ui::theme::TEXT
                } else {
                    crate::ui::theme::TEXT_MUTED
                };
                let stroke = egui::Stroke::new(1.2_f32, dismiss_color);
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
    let stroke = egui::Stroke::new(1.4_f32, color);
    painter.circle_stroke(center - egui::vec2(r * 0.3, r * 0.3), r, stroke);
    painter.line_segment(
        [
            center + egui::vec2(r * 0.4, r * 0.4),
            center + egui::vec2(r * 1.1, r * 1.1),
        ],
        stroke,
    );
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
            left: crate::ui::theme::SPACE_2 as i8,
            right: crate::ui::theme::SPACE_2 as i8,
            top: crate::ui::theme::SPACE_3 as i8,
            bottom: crate::ui::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            selected_recent_search = paint_palette_empty_group(
                ui,
                "Recent",
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
            left: crate::ui::theme::SPACE_4 as i8,
            right: crate::ui::theme::SPACE_4 as i8,
            top: crate::ui::theme::SPACE_6 as i8,
            bottom: crate::ui::theme::SPACE_6 as i8,
        })
        .show(ui, |ui| {
            ui.add_sized(
                egui::vec2(ui.available_width(), 16.0),
                egui::Label::new(
                    egui::RichText::new(text)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::ui::theme::TEXT_SUBTLE)
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
            left: crate::ui::theme::SPACE_4 as i8,
            right: crate::ui::theme::SPACE_4 as i8,
            top: crate::ui::theme::SPACE_4 as i8,
            bottom: crate::ui::theme::SPACE_4 as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::ui::theme::SPACE_3);
            ui.label(
                egui::RichText::new("Semantic Search requires a local download.")
                    .text_style(egui::TextStyle::Body)
                    .color(crate::ui::theme::TEXT),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Install the search assets to find references by description.",
                    )
                    .text_style(egui::TextStyle::Small)
                    .color(crate::ui::theme::TEXT_MUTED),
                )
                .wrap(),
            );
            if let SearchOverlayStatus::Error(message) = status {
                ui.label(
                    egui::RichText::new(message)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::ui::theme::DESTRUCTIVE),
                );
            }
            let label = match status {
                SearchOverlayStatus::Error(_) => "Retry",
                SearchOverlayStatus::Loading(message) => message.as_str(),
                _ => "Install",
            };
            let button = egui::Button::new(label)
                .min_size(egui::vec2(112.0, 30.0))
                .corner_radius(crate::ui::theme::RADIUS_CONTROL);
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
            left: crate::ui::theme::SPACE_2 as i8,
            right: crate::ui::theme::SPACE_2 as i8,
            top: crate::ui::theme::SPACE_1 as i8,
            bottom: crate::ui::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.label(crate::ui::theme::editorial_caption(title));
        });
    for (index, row) in rows.iter().enumerate() {
        let row_height = 34.0;
        let row_width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(row_width, row_height), egui::Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let active = index == active_recent_search_index;
        let hover = crate::ui::motion::hover_amount(
            ui.ctx(),
            response.id.with("recent-hover"),
            response.hovered() && !active,
        );
        let press = crate::ui::motion::press_amount(
            ui.ctx(),
            response.id.with("recent-press"),
            response.is_pointer_button_down_on(),
        );
        let fill_rect = crate::ui::motion::pressed_rect(rect.shrink2(egui::vec2(2.0, 2.0)), press);
        if active {
            ui.painter().rect_filled(
                fill_rect,
                crate::ui::theme::RADIUS_COMPACT,
                crate::ui::motion::mix(
                    crate::ui::theme::SURFACE_ELEVATED,
                    crate::ui::theme::SURFACE_PRESSED,
                    press * 0.65,
                ),
            );
        } else if hover > 0.0 || press > 0.0 {
            ui.painter().rect_filled(
                fill_rect,
                crate::ui::theme::RADIUS_COMPACT,
                crate::ui::motion::mix(
                    crate::ui::theme::SURFACE.gamma_multiply(hover),
                    crate::ui::theme::SURFACE_PRESSED,
                    press * 0.65,
                ),
            );
        }
        ui.painter().text(
            egui::pos2(rect.left() + 14.0, rect.center().y + press * 0.5),
            egui::Align2::LEFT_CENTER,
            row.as_str(),
            egui::TextStyle::Body.resolve(ui.style()),
            crate::ui::theme::TEXT,
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
) {
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(38))
        .inner_margin(egui::Margin {
            left: crate::ui::theme::SPACE_4 as i8,
            right: crate::ui::theme::SPACE_4 as i8,
            top: PALETTE_CHROME_Y_MARGIN,
            bottom: PALETTE_CHROME_Y_MARGIN,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(CHROME_CELL_SIZE.y);
                ui.spacing_mut().item_spacing.x = shortcut::HINT_GAP;
                if result_count == 0 {
                    let idle_text = palette_status_text(status, "Ready");
                    paint_palette_footer_status(ui, &idle_text);
                } else if has_more_results {
                    shortcut::hint(ui, &[Key::Down], "More");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if result_count == 0 {
                        shortcut::hint(ui, &[Key::Enter], "Select");
                        shortcut::hint(ui, &[Key::Up, Key::Down], "Navigate");
                    } else {
                        shortcut::hint(ui, &[Key::Command, Key::Enter], "Place all");
                        shortcut::hint(ui, &[Key::Enter], "Place");
                    }
                });
            });
        });
}

/// A status dot and the search state, on the hint label's type.
fn paint_palette_footer_status(ui: &mut egui::Ui, label: &str) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(PALETTE_FOOTER_STATUS_WIDTH, CHROME_CELL_SIZE.y),
        egui::Sense::hover(),
    );
    ui.painter().circle_filled(
        rect.left_center() + egui::vec2(3.0, 0.0),
        3.0,
        crate::ui::theme::GREEN,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(6.0 + crate::ui::theme::SPACE_2, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(crate::ui::theme::TYPE_DETAIL),
        crate::ui::theme::TEXT_MUTED,
    );
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
    use super::{
        PALETTE_WIDTH, search_result_grid_content_height, search_result_grid_layout,
        search_result_tile_height,
    };
    use crate::ui::theme::SPACE_2;

    #[test]
    fn search_result_grid_layout_keeps_partial_rows_at_five_column_width() {
        let shell_content_width = PALETTE_WIDTH - 24.0;
        let five_result_layout = search_result_grid_layout(shell_content_width, 5);
        let two_result_layout = search_result_grid_layout(shell_content_width, 2);

        assert_eq!(two_result_layout.columns, 5);
        assert_eq!(two_result_layout.tile_width, five_result_layout.tile_width);
    }

    #[test]
    fn search_result_grid_content_height_grows_by_rows() {
        let height = search_result_grid_content_height(6, 5);

        assert_eq!(height, search_result_tile_height() * 2.0 + SPACE_2);
    }
}
