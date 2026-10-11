use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use eframe::egui;

use crate::canvas::interaction::{
    hit_test_textbox_resize_handle, textbox_screen_rect, textbox_world_rect,
};
use crate::canvas::{CanvasState, MIN_CANVAS_ZOOM, TextboxLayoutCacheKey, TextboxPointerRegion};
use crate::data::{CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind, CanvasTextSize};
use crate::ui::theme;

/// Label proportions in ems of its font size. Every length follows the font,
/// so a label keeps its shape at every zoom. The radius matches
/// `RADIUS_COMPACT` against `TYPE_CONTROL`.
const LABEL_PAD_X_EM: f32 = 0.7;
const LABEL_PAD_Y_EM: f32 = 0.34;
pub(crate) const LABEL_RADIUS_EM: f32 = 0.46;
/// An empty label keeps room for the caret.
const LABEL_EMPTY_WIDTH_EM: f32 = 2.0;

/// Label font size in world units against the 300-unit placement span.
pub(crate) fn textbox_font_size(text_size: CanvasTextSize) -> f32 {
    let baseline = crate::placement::placement_target_reference_span();
    match text_size {
        CanvasTextSize::S => baseline * (11.0 / 300.0),
        CanvasTextSize::M => baseline * (16.0 / 300.0),
        CanvasTextSize::L => baseline * (24.0 / 300.0),
        CanvasTextSize::Xl => baseline * (36.0 / 300.0),
        CanvasTextSize::Xxl => baseline * (56.0 / 300.0),
    }
}

/// Where the text starts inside a label, in world units.
pub(crate) fn label_text_offset(text_size: CanvasTextSize) -> egui::Vec2 {
    egui::vec2(LABEL_PAD_X_EM, LABEL_PAD_Y_EM) * textbox_font_size(text_size)
}

/// A label's fill and the ink that reads on it. A colored label is filled
/// with its color so it shows from across the board.
pub(crate) fn label_colors(color: CanvasAnnotationColor) -> (egui::Color32, egui::Color32) {
    if color == CanvasAnnotationColor::Default {
        (theme::TOOLTIP, theme::TEXT)
    } else {
        (
            super::annotations::annotation_color(color),
            theme::BACKGROUND,
        )
    }
}

/// World size of a label that fits `text`. Labels never wrap; each line of
/// text is one row.
pub(crate) fn textbox_size_for_text(
    ctx: &egui::Context,
    text: &str,
    text_size: CanvasTextSize,
) -> egui::Vec2 {
    let em = textbox_font_size(text_size);
    let galley = ctx.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            if text.is_empty() { " " } else { text }.to_owned(),
            egui::FontId::proportional(em),
            egui::Color32::WHITE,
        )
    });
    let mut text_extent = galley.size();
    if text.is_empty() {
        text_extent.x = LABEL_EMPTY_WIDTH_EM * em;
    }
    text_extent + 2.0 * label_text_offset(text_size)
}

/// Keeps a label's stored rect equal to its text. The top-left stays put.
pub(crate) fn fit_label_to_text(ctx: &egui::Context, state: &mut CanvasState, index: usize) {
    let annotation = &state.document.annotations[index];
    if annotation.kind != CanvasAnnotationKind::Textbox {
        return;
    }
    let size = textbox_size_for_text(ctx, &annotation.text, annotation.text_size);
    let world_rect = textbox_world_rect(annotation);
    if (world_rect.size() - size).abs().max_elem() < 0.01 {
        return;
    }
    let annotation_id = annotation.id;
    let annotation = &mut state.document.annotations[index];
    annotation.geometry = crate::data::CanvasAnnotationGeometry::from_kind(
        annotation.kind,
        world_rect.min,
        world_rect.min + size,
    );
    state.mark_annotation_dirty(annotation_id);
}

pub(crate) fn textbox_font_id(text_size: CanvasTextSize, zoom: f32) -> egui::FontId {
    egui::FontId::proportional(textbox_font_size(text_size) * zoom.max(MIN_CANVAS_ZOOM))
}

/// Screen rect of the text inside a label.
fn label_text_screen_rect(
    state: &CanvasState,
    rect: egui::Rect,
    annotation: &CanvasAnnotation,
    galley: &egui::Galley,
) -> egui::Rect {
    let chip = textbox_screen_rect(state, rect, annotation);
    let min = chip.min + label_text_offset(annotation.text_size) * state.zoom;
    egui::Rect::from_min_size(min, galley.size())
}

pub(crate) fn textbox_pointer_region(
    ui: &egui::Ui,
    rect: egui::Rect,
    state: &mut CanvasState,
    annotation: &CanvasAnnotation,
    pointer_pos: egui::Pos2,
) -> Option<TextboxPointerRegion> {
    debug_assert!(annotation.kind.is_rect());

    let screen_rect = textbox_screen_rect(state, rect, annotation);
    if annotation.kind == CanvasAnnotationKind::Rectangle {
        if let Some(handle) = hit_test_textbox_resize_handle(state, rect, annotation, pointer_pos) {
            return Some(TextboxPointerRegion::Resize(handle));
        }
        return screen_rect
            .contains(pointer_pos)
            .then_some(TextboxPointerRegion::Body);
    }

    // Labels size themselves to their text, so they have no resize edges.
    if !screen_rect.contains(pointer_pos) {
        return None;
    }
    if state.interaction.editing_text_annotation_id == Some(annotation.id) {
        let layout = state.cached_textbox_layout(
            ui,
            annotation.id,
            &annotation.text,
            annotation.text_size,
            annotation.color,
            f32::INFINITY,
            state.zoom,
        );
        if label_text_screen_rect(state, rect, annotation, &layout.galley).contains(pointer_pos) {
            return Some(TextboxPointerRegion::Text);
        }
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
        label_colors(color).1,
        wrap_width.max(24.0),
    );
    ui.fonts_mut(|fonts| fonts.layout_job(job))
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
    let annotation = &state.document.annotations[index];
    let chip = textbox_screen_rect(state, rect, annotation);
    let text_pos = chip.min + label_text_offset(annotation.text_size) * state.zoom;
    let text_size = annotation.text_size;
    let text_color = annotation.color;
    let zoom = state.zoom;
    let clip_rect = state
        .member_clip_screen_rect(rect, annotation_id)
        .map_or(rect, |clip| clip.intersect(rect));

    egui::Area::new(egui::Id::new(("textbox-editor", annotation_id)))
        .order(egui::Order::Foreground)
        .fixed_pos(text_pos)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(clip_rect);
            // Enter commits a label; Shift+Enter starts a new line. The Enter
            // that opened the editor does not also close it.
            let opening = state.interaction.focus_text_editor;
            let commit = !opening
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _width: f32| {
                layout_textbox_galley(
                    ui,
                    text.as_str(),
                    text_size,
                    text_color,
                    f32::INFINITY,
                    zoom,
                )
            };
            let output = egui::TextEdit::multiline(&mut state.document.annotations[index].text)
                .desired_width(1.0)
                .desired_rows(1)
                .font(textbox_font_id(text_size, zoom))
                .layouter(&mut layouter)
                .return_key(Some(egui::KeyboardShortcut::new(
                    egui::Modifiers::SHIFT,
                    egui::Key::Enter,
                )))
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .show(ui);
            let text = state.document.annotations[index].text.clone();
            state.cache_textbox_layout(
                annotation_id,
                &text,
                text_size,
                text_color,
                f32::INFINITY,
                zoom,
                output.galley.clone(),
            );
            if state.interaction.focus_text_editor {
                output.response.request_focus();
                state.interaction.focus_text_editor = false;
            }
            if output.response.changed() {
                fit_label_to_text(ui.ctx(), state, index);
                state.mark_annotation_dirty(annotation_id);
            }
            if commit
                || (output.response.has_focus()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
                || output.response.lost_focus()
            {
                state.finish_textbox_edit();
            }
        });
}
