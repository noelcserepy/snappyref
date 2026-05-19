use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use eframe::egui;

use super::annotations::annotation_color;
use crate::canvas::interaction::{
    annotation_screen_rect, hit_test_textbox_resize_handle, normalized_textbox_rect,
    textbox_screen_rect,
};
use crate::canvas::{
    CanvasState, MIN_CANVAS_ZOOM, MIN_TEXTBOX_WORLD_SIZE, TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP,
    TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH, TEXTBOX_EDITOR_TOOLBAR_CLEARANCE,
    TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN, TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH,
    TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH, TextboxLayoutCacheKey, TextboxPointerRegion,
};
use crate::data::{
    CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasStrokeWidth,
    CanvasTextSize,
};

pub(crate) fn textbox_padding() -> egui::Vec2 {
    egui::vec2(crate::ui::theme::SPACE_2, crate::ui::theme::SPACE_2)
}

pub(crate) fn textbox_font_size(text_size: CanvasTextSize) -> f32 {
    let baseline = crate::jobs::placement_target_reference_span();
    match text_size {
        CanvasTextSize::S => baseline * (16.0 / 300.0),
        CanvasTextSize::M => baseline * (20.0 / 300.0),
        CanvasTextSize::L => baseline * (40.0 / 300.0),
        CanvasTextSize::Xl => baseline * (64.0 / 300.0),
    }
}

pub(crate) fn textbox_font_id(text_size: CanvasTextSize, zoom: f32) -> egui::FontId {
    egui::FontId::proportional(textbox_font_size(text_size) * zoom.max(MIN_CANVAS_ZOOM))
}

pub(crate) fn textbox_content_rect(textbox_rect: egui::Rect) -> egui::Rect {
    let padding = textbox_padding();
    let min = egui::pos2(
        (textbox_rect.min.x + padding.x).min(textbox_rect.max.x),
        (textbox_rect.min.y + padding.y).min(textbox_rect.max.y),
    );
    let available_size = (textbox_rect.max - min).max(egui::Vec2::ZERO);
    let desired_size = (textbox_rect.size() - padding * 2.0).max(egui::Vec2::ZERO);
    egui::Rect::from_min_size(
        min,
        egui::vec2(
            desired_size.x.min(available_size.x),
            desired_size.y.min(available_size.y),
        ),
    )
}

fn textbox_row_height(ui: &egui::Ui, text_size: CanvasTextSize, zoom: f32) -> f32 {
    let font_id = textbox_font_id(text_size, zoom);
    ui.fonts_mut(|fonts| fonts.row_height(&font_id))
}

pub(crate) fn textbox_empty_hotspot_rect(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    text_size: CanvasTextSize,
    zoom: f32,
) -> egui::Rect {
    let width = content_rect.width().clamp(
        TEXTBOX_EMPTY_HOTSPOT_MIN_WIDTH,
        TEXTBOX_EMPTY_HOTSPOT_MAX_WIDTH,
    );
    let size = egui::vec2(
        width,
        textbox_row_height(ui, text_size, zoom).min(content_rect.height()),
    );
    egui::Rect::from_min_size(content_rect.min, size.min(content_rect.size()))
}

pub(crate) fn textbox_text_interactive_rect_from_galley(
    ui: &egui::Ui,
    content_rect: egui::Rect,
    galley: &egui::Galley,
    text_size: CanvasTextSize,
    zoom: f32,
    is_empty: bool,
) -> egui::Rect {
    if is_empty {
        return textbox_empty_hotspot_rect(ui, content_rect, text_size, zoom);
    }

    egui::Rect::from_min_size(content_rect.min, galley.size()).intersect(content_rect)
}

pub(crate) fn textbox_pointer_region(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> Option<TextboxPointerRegion> {
    debug_assert_eq!(annotation.kind, CanvasAnnotationKind::Textbox);

    let editing = state.interaction.editing_text_annotation_id == Some(annotation.id);
    let textbox_rect = textbox_screen_rect(state, rect, annotation);
    if !textbox_rect.contains(pointer_pos) {
        return hit_test_textbox_resize_handle(state, rect, annotation, pointer_pos)
            .map(TextboxPointerRegion::Resize);
    }

    if editing {
        let content_rect = textbox_content_rect(textbox_rect);
        let layout = state.cached_textbox_layout(
            ui,
            annotation.id,
            &annotation.text,
            annotation.text_size,
            annotation.color,
            content_rect.width(),
            state.zoom,
        );
        let text_rect = textbox_text_interactive_rect_from_galley(
            ui,
            content_rect,
            &layout.galley,
            annotation.text_size,
            state.zoom,
            annotation.text.is_empty(),
        );
        if text_rect.contains(pointer_pos) {
            return Some(TextboxPointerRegion::Text);
        }
    }

    if let Some(handle) = hit_test_textbox_resize_handle(state, rect, annotation, pointer_pos) {
        return Some(TextboxPointerRegion::Resize(handle));
    }

    Some(TextboxPointerRegion::Body)
}

pub(crate) fn textbox_layout_cache_key(
    text: &str,
    text_size: CanvasTextSize,
    color: CanvasAnnotationColor,
    wrap_width: f32,
    zoom: f32,
) -> TextboxLayoutCacheKey {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    TextboxLayoutCacheKey {
        text_hash: hasher.finish(),
        text_size,
        color,
        wrap_width_bits: wrap_width.max(24.0).to_bits(),
        zoom_bits: zoom.to_bits(),
    }
}

pub(crate) fn layout_textbox_galley(
    ui: &egui::Ui,
    text: &str,
    text_size: CanvasTextSize,
    color: CanvasAnnotationColor,
    wrap_width: f32,
    zoom: f32,
) -> Arc<egui::Galley> {
    let job = egui::text::LayoutJob::simple(
        if text.is_empty() {
            " ".to_owned()
        } else {
            text.to_owned()
        },
        textbox_font_id(text_size, zoom),
        annotation_color(color),
        wrap_width.max(24.0),
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub(crate) fn textbox_required_world_height(galley_height_screen: f32, zoom: f32) -> f32 {
    let padding = textbox_padding();
    let screen_height = galley_height_screen + padding.y * 2.0;
    (screen_height / zoom.max(MIN_CANVAS_ZOOM)).max(MIN_TEXTBOX_WORLD_SIZE.y)
}
struct TextboxEditorToolbarMetrics {
    size: egui::Vec2,
}

const ANNOTATION_COLORS: [CanvasAnnotationColor; 5] = [
    CanvasAnnotationColor::Default,
    CanvasAnnotationColor::Coral,
    CanvasAnnotationColor::Gold,
    CanvasAnnotationColor::Mint,
    CanvasAnnotationColor::Sky,
];
const TOOLBAR_SWATCH_BUTTON_WIDTH: f32 = 28.0;
const TOOLBAR_BACKGROUND_BUTTON_WIDTH: f32 = 36.0;

fn expand_textbox_to_height(state: &mut CanvasState, index: usize, required_height: f32) -> bool {
    let annotation = &state.document.annotations[index];
    if annotation.kind != CanvasAnnotationKind::Textbox {
        return false;
    }

    let world_rect = normalized_textbox_rect(
        egui::pos2(annotation.start().x, annotation.start().y),
        egui::pos2(annotation.end().x, annotation.end().y),
    );
    if required_height <= world_rect.height() + 0.5 {
        return false;
    }

    let annotation_id = annotation.id;
    let new_rect = egui::Rect::from_min_size(
        world_rect.min,
        egui::vec2(world_rect.width(), required_height),
    );
    let annotation = &mut state.document.annotations[index];
    annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
        annotation.kind,
        new_rect.min,
        new_rect.max,
    );
    state.mark_annotation_dirty(annotation_id);
    true
}
pub(crate) fn show_textbox_editor(ui: &egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let Some(annotation_id) = state.interaction.editing_text_annotation_id else {
        return;
    };
    let Some(index) = state
        .document
        .annotations
        .iter()
        .position(|annotation| annotation.id == annotation_id)
    else {
        state.interaction.editing_text_annotation_id = None;
        state.interaction.focus_text_editor = false;
        return;
    };
    let textbox_rect = annotation_screen_rect(state, rect, &state.document.annotations[index]);
    let toolbar_rect = editing_textbox_toolbar_rect(ui, state, rect);
    let content_rect = textbox_content_rect(textbox_rect);
    let text = state.document.annotations[index].text.clone();
    let layout = state.cached_textbox_layout(
        ui,
        annotation_id,
        &text,
        state.document.annotations[index].text_size,
        state.document.annotations[index].color,
        content_rect.width(),
        state.zoom,
    );
    let text_rect = textbox_text_interactive_rect_from_galley(
        ui,
        content_rect,
        &layout.galley,
        state.document.annotations[index].text_size,
        state.zoom,
        text.is_empty(),
    );
    let toolbar_interacting = toolbar_rect
        .map(|toolbar_rect| show_textbox_editor_toolbar(ui, toolbar_rect, state))
        .unwrap_or(false);

    egui::Area::new(egui::Id::new(("textbox-editor", annotation_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(text_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(content_rect);
            let zoom = state.zoom;
            let text_size = state.document.annotations[index].text_size;
            let text_color = state.document.annotations[index].color;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                let wrap_width = if content_rect.width() > 0.0 {
                    content_rect.width()
                } else {
                    width
                };
                layout_textbox_galley(ui, text.as_str(), text_size, text_color, wrap_width, zoom)
            };
            let output = ui
                .allocate_ui(text_rect.size(), |ui| {
                    ui.set_min_size(text_rect.size());
                    ui.visuals_mut().override_text_color = Some(annotation_color(text_color));
                    egui::TextEdit::multiline(&mut state.document.annotations[index].text)
                        .desired_width(content_rect.width())
                        .font(textbox_font_id(text_size, state.zoom))
                        .layouter(&mut layouter)
                        .frame(egui::Frame::NONE)
                        .show(ui)
                })
                .inner;
            let text = state.document.annotations[index].text.clone();
            let entry = state.cache_textbox_layout(
                annotation_id,
                &text,
                text_size,
                text_color,
                content_rect.width(),
                state.zoom,
                output.galley.clone(),
            );
            let required_height = entry.required_world_height;
            expand_textbox_to_height(state, index, required_height);
            if state.interaction.focus_text_editor {
                output.response.request_focus();
                state.interaction.focus_text_editor = false;
            }
            if output.response.changed() {
                state.mark_annotation_dirty(annotation_id);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) && output.response.has_focus() {
                state.finish_textbox_edit();
            } else if output.response.lost_focus() && !toolbar_interacting {
                state.finish_textbox_edit();
            }
        });
}

fn textbox_editor_toolbar_metrics(_ui: &egui::Ui) -> TextboxEditorToolbarMetrics {
    let button_span = TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH * 4.0
        + TOOLBAR_SWATCH_BUTTON_WIDTH * 5.0
        + TOOLBAR_BACKGROUND_BUTTON_WIDTH
        + TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP * 9.0;
    TextboxEditorToolbarMetrics {
        size: egui::vec2(button_span, crate::ui::theme::CONTROL_HEIGHT),
    }
}

fn annotation_style_toolbar_metrics(_ui: &egui::Ui) -> TextboxEditorToolbarMetrics {
    let button_span = TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH * 3.0
        + TOOLBAR_SWATCH_BUTTON_WIDTH * 5.0
        + TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP * 7.0;
    TextboxEditorToolbarMetrics {
        size: egui::vec2(button_span, crate::ui::theme::CONTROL_HEIGHT),
    }
}

pub(crate) fn editing_textbox_toolbar_rect(
    ui: &egui::Ui,
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    let annotation_id = state.interaction.editing_text_annotation_id?;
    let annotation = state.annotation_by_id(annotation_id)?;
    if annotation.kind != CanvasAnnotationKind::Textbox {
        return None;
    }

    let textbox_rect = annotation_screen_rect(state, rect, annotation);
    let metrics = textbox_editor_toolbar_metrics(ui);
    let min_x = rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let max_x = (rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - metrics.size.x).max(min_x);
    let preferred_x = textbox_rect.left();
    let x = preferred_x.clamp(min_x, max_x);

    let min_y = rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let above_y = textbox_rect.top() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE - metrics.size.y;
    let y = above_y.max(min_y);

    Some(egui::Rect::from_min_size(egui::pos2(x, y), metrics.size))
}

fn show_textbox_editor_toolbar(
    ui: &egui::Ui,
    toolbar_rect: egui::Rect,
    state: &mut CanvasState,
) -> bool {
    let selected = state.editing_textbox_size();
    let selected_color = state.editing_textbox_color();
    let background_enabled = state.editing_textbox_background_enabled().unwrap_or(false);
    let mut next_size = None;
    let mut next_color = None;
    let mut toggle_background = false;
    let pointer_interacting = ui.input(|input| {
        input.pointer.latest_pos().is_some_and(|pointer_pos| {
            toolbar_rect.contains(pointer_pos)
                && (input.pointer.any_pressed()
                    || input.pointer.any_down()
                    || input.pointer.any_released())
        })
    });

    egui::Area::new(egui::Id::new("textbox-editor-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(toolbar_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(toolbar_rect.size());
            ui.spacing_mut().item_spacing.x = TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP;
            ui.horizontal(|ui| {
                for size in [
                    CanvasTextSize::S,
                    CanvasTextSize::M,
                    CanvasTextSize::L,
                    CanvasTextSize::Xl,
                ] {
                    let button = egui::Button::new(
                        egui::RichText::new(size.label())
                            .text_style(egui::TextStyle::Button)
                            .color(crate::ui::theme::TEXT),
                    )
                    .fill(if selected == Some(size) {
                        crate::ui::theme::SURFACE_ELEVATED
                    } else {
                        crate::ui::theme::SURFACE
                    })
                    .stroke(if selected == Some(size) {
                        crate::ui::theme::stroke_strong()
                    } else {
                        crate::ui::theme::stroke_subtle()
                    })
                    .corner_radius(crate::ui::theme::RADIUS_CONTROL)
                    .min_size(egui::vec2(
                        TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH,
                        crate::ui::theme::CONTROL_HEIGHT,
                    ));
                    if ui.add(button).clicked() {
                        next_size = Some(size);
                    }
                }
                for color in ANNOTATION_COLORS {
                    if paint_color_swatch_button(ui, color, selected_color == Some(color)).clicked()
                    {
                        next_color = Some(color);
                    }
                }
                let background_button = egui::Button::new(
                    egui::RichText::new("BG").text_style(egui::TextStyle::Button),
                )
                .fill(if background_enabled {
                    crate::ui::theme::SURFACE_ELEVATED
                } else {
                    crate::ui::theme::SURFACE
                })
                .stroke(if background_enabled {
                    crate::ui::theme::stroke_strong()
                } else {
                    crate::ui::theme::stroke_subtle()
                })
                .corner_radius(crate::ui::theme::RADIUS_CONTROL)
                .min_size(egui::vec2(
                    TOOLBAR_BACKGROUND_BUTTON_WIDTH,
                    crate::ui::theme::CONTROL_HEIGHT,
                ));
                if ui.add(background_button).clicked() {
                    toggle_background = true;
                }
            });
        });

    let changed = next_size
        .map(|text_size| state.apply_text_size_to_editing_textbox(text_size))
        .unwrap_or(false)
        || next_color
            .map(|color| state.apply_color_to_editing_textbox(color))
            .unwrap_or(false)
        || (toggle_background && state.toggle_editing_textbox_background());
    pointer_interacting || changed
}

pub(crate) fn annotation_style_toolbar_rect(
    ui: &egui::Ui,
    state: &CanvasState,
    rect: egui::Rect,
) -> Option<egui::Rect> {
    let annotation = state.selected_annotation()?;
    if !matches!(
        annotation.kind,
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
    ) {
        return None;
    }

    let annotation_rect = annotation_screen_rect(state, rect, annotation);
    let metrics = annotation_style_toolbar_metrics(ui);
    let min_x = rect.left() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let max_x = (rect.right() - TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN - metrics.size.x).max(min_x);
    let x = annotation_rect.left().clamp(min_x, max_x);
    let min_y = rect.top() + TEXTBOX_EDITOR_TOOLBAR_VIEWPORT_MARGIN;
    let y = (annotation_rect.top() - TEXTBOX_EDITOR_TOOLBAR_CLEARANCE - metrics.size.y).max(min_y);
    Some(egui::Rect::from_min_size(egui::pos2(x, y), metrics.size))
}

pub(crate) fn show_annotation_style_toolbar(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
) {
    let Some(toolbar_rect) = annotation_style_toolbar_rect(ui, state, rect) else {
        return;
    };
    let selected_width = state.selected_stroke_width();
    let selected_color = state.selected_annotation_color();
    let mut next_width = None;
    let mut next_color = None;

    egui::Area::new(egui::Id::new("annotation-style-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(toolbar_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(toolbar_rect.size());
            ui.spacing_mut().item_spacing.x = TEXTBOX_EDITOR_TOOLBAR_BUTTON_GAP;
            ui.horizontal(|ui| {
                for width in [
                    CanvasStrokeWidth::S,
                    CanvasStrokeWidth::M,
                    CanvasStrokeWidth::L,
                ] {
                    let button = egui::Button::new(width.label())
                        .fill(if selected_width == Some(width) {
                            crate::ui::theme::SURFACE_ELEVATED
                        } else {
                            crate::ui::theme::SURFACE
                        })
                        .stroke(if selected_width == Some(width) {
                            crate::ui::theme::stroke_strong()
                        } else {
                            crate::ui::theme::stroke_subtle()
                        })
                        .corner_radius(crate::ui::theme::RADIUS_CONTROL)
                        .min_size(egui::vec2(
                            TEXTBOX_EDITOR_TOOLBAR_BUTTON_WIDTH,
                            crate::ui::theme::CONTROL_HEIGHT,
                        ));
                    if ui.add(button).clicked() {
                        next_width = Some(width);
                    }
                }
                for color in ANNOTATION_COLORS {
                    if paint_color_swatch_button(ui, color, selected_color == Some(color)).clicked()
                    {
                        next_color = Some(color);
                    }
                }
            });
        });

    if let Some(width) = next_width {
        state.apply_stroke_width_to_selected_annotation(width);
    }
    if let Some(color) = next_color {
        state.apply_color_to_selected_annotation(color);
    }
}

fn paint_color_swatch_button(
    ui: &mut egui::Ui,
    color: CanvasAnnotationColor,
    selected: bool,
) -> egui::Response {
    ui.add(
        egui::Button::new("")
            .fill(annotation_color(color))
            .stroke(if selected {
                crate::ui::theme::stroke_strong()
            } else {
                crate::ui::theme::stroke_subtle()
            })
            .corner_radius(crate::ui::theme::RADIUS_CONTROL)
            .min_size(egui::vec2(
                TOOLBAR_SWATCH_BUTTON_WIDTH,
                crate::ui::theme::CONTROL_HEIGHT,
            )),
    )
}
