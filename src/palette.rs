use std::time::Instant;

use eframe::egui;

use crate::semantic_search::SearchResultGroup;

pub(crate) const SEARCH_DEBOUNCE_MS: f32 = 120.0;
pub(crate) const SEARCH_OVERLAY_TOP_MARGIN: f32 = 56.0;
pub(crate) const PALETTE_WIDTH: f32 = 660.0;
pub(crate) const PALETTE_GRID_COLUMNS: usize = 5;
pub(crate) const PALETTE_GRID_MAX_HEIGHT: f32 = 360.0;

const SEARCH_RESULT_TILE_PREVIEW_HEIGHT: f32 = 144.0;

#[derive(Default)]
pub(crate) struct SearchOverlayState {
    pub open: bool,
    pub query: String,
    pub last_submitted_query: String,
    pub last_query_edit_at: Option<Instant>,
    pub active_result_index: usize,
    pub active_instance_index: usize,
    pub selected_result_indices: Vec<usize>,
    pub selection_anchor_index: Option<usize>,
    pub drag_state: Option<SearchResultDragState>,
    pub last_expanded_overlay_rect: Option<egui::Rect>,
    pub results: Vec<SearchResultGroup>,
    pub status: SearchOverlayStatus,
    pub focus_requested: bool,
    pub image_pivot: Option<ImageSearchPivot>,
}

#[derive(Clone, Debug)]
pub(crate) struct SearchResultDragState {
    pub item_ids: Vec<i64>,
    pub outside_overlay: bool,
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
    let columns = PALETTE_GRID_COLUMNS.min(result_count.max(1));
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
    active: bool,
    selected: bool,
    tile_width: f32,
) -> egui::Response {
    let total_size = egui::vec2(tile_width, search_result_tile_height());
    let (rect, response) = ui.allocate_exact_size(total_size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let tile_radius = if active || selected {
        0
    } else {
        crate::theme::CARD_RADIUS
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
        let preview_aspect = preview_rect.width() / preview_rect.height().max(1.0);
        let texture_aspect = preview.texture_size.x / preview.texture_size.y.max(1.0);
        let uv_rect = if texture_aspect > preview_aspect {
            let visible_u = preview_aspect / texture_aspect;
            let inset = (1.0 - visible_u) * 0.5;
            egui::Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
        } else {
            let visible_v = texture_aspect / preview_aspect;
            let inset = (1.0 - visible_v) * 0.5;
            egui::Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
        };
        ui.painter().image(
            preview.texture_id,
            preview_rect,
            uv_rect,
            egui::Color32::WHITE,
        );
    } else {
        ui.painter()
            .rect_filled(preview_rect, tile_radius, crate::theme::BACKGROUND);
    }

    if selected || active {
        let stroke = if active {
            crate::theme::stroke_strong()
        } else {
            egui::Stroke::new(1.0, crate::theme::BORDER_STRONG)
        };
        ui.painter()
            .rect_stroke(preview_rect, tile_radius, stroke, egui::StrokeKind::Outside);
    }

    response
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
            left: 18,
            right: 14,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let placeholder = if let Some(pivot) = pivot {
                    if paint_image_pivot_pill(ui, canvas, pivot) {
                        pivot_dismissed = true;
                    }
                    "Refine with a term..."
                } else {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    paint_search_icon(ui, icon_rect, crate::theme::TEXT_SUBTLE);
                    "Search images, tags, boards"
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

                let resp = ui.add(
                    egui::TextEdit::singleline(query)
                        .id(input_id)
                        .desired_width(ui.available_width() - 44.0)
                        .hint_text(placeholder)
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
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

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
                        egui::CornerRadius::same(3),
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

    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(15))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(15)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin {
            left: 5,
            right: 5,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Monospace)
                    .color(crate::theme::TEXT_MUTED)
                    .size(label_size),
            );
        });
}

fn kbd_label_uses_small_symbol_glyphs(label: &str) -> bool {
    label.chars().any(|ch| matches!(ch, '⇧' | '↵'))
}

pub(crate) fn paint_palette_divider(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0, egui::Color32::from_white_alpha(14));
}

pub(crate) fn paint_palette_empty_state(ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 8,
            right: 8,
            top: 12,
            bottom: 6,
        })
        .show(ui, |ui| {
            paint_palette_empty_group(ui, "RECENT", &["boat", "forest path", "still life"]);
        });
}

fn paint_palette_empty_group(ui: &mut egui::Ui, title: &str, rows: &[&str]) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 6,
            right: 6,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .size(9.0),
            );
        });
    for row in rows {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 6,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(*row)
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT),
                );
            });
    }
    ui.add_space(4.0);
}

pub(crate) fn paint_palette_footer(
    ui: &mut egui::Ui,
    result_count: usize,
    status: &SearchOverlayStatus,
) {
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(38))
        .inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 9,
            bottom: 9,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if result_count == 0 {
                    let idle_text = match status {
                        SearchOverlayStatus::Idle => "Ready".to_owned(),
                        SearchOverlayStatus::Indexing => "Indexing…".to_owned(),
                        SearchOverlayStatus::Loading(msg) => msg.clone(),
                        SearchOverlayStatus::Ready => "Ready".to_owned(),
                        SearchOverlayStatus::Error(err) => err.clone(),
                    };
                    let dot_rect = ui
                        .allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover())
                        .0;
                    ui.painter()
                        .circle_filled(dot_rect.center(), 3.0, crate::theme::INDEXED_DOT);
                    ui.label(
                        egui::RichText::new(idle_text)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(
                        egui::RichText::new("board")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                    paint_kbd_pill(ui, "↵");
                    paint_kbd_pill(ui, "⌘");
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new("open")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE)
                            .size(11.0),
                    );
                    paint_kbd_pill(ui, "↵");
                });
            });
        });
}
