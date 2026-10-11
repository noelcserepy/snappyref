//! Annotation style by keyboard. With annotations selected, 1 to 5 pick a
//! color, Shift+1 to 5 pick each one's size, H switches a stroke's head, and
//! F steps a rectangle's fill. With nothing selected, the color, size, and
//! fill keys set the style the next annotation starts in. Every change also
//! becomes that next style.
//!
//! Nothing floats next to the selection. A quiet key row sits bottom-center
//! while annotations are selected, and a HUD takes its place for a moment
//! after each change, showing the whole scale with the new value lit.

use eframe::egui;

use super::CanvasState;
use super::render::{annotation_color, stroke_width_value};
use crate::data::{
    CanvasAnnotationColor, CanvasAnnotationKind, CanvasRectangleFill, CanvasStrokeWidth,
    CanvasTextSize,
};
use crate::ui::shortcut::{self, Key, Shortcut};
use crate::ui::{motion, theme};

/// The style keys, shown in the key row and in Settings → Keys.
pub(crate) const COLOR_KEYS: Shortcut = &[Key::Named("1–5")];
pub(crate) const SIZE_KEYS: Shortcut = &[Key::Shift, Key::Named("1–5")];
pub(crate) const HEAD_KEYS: Shortcut = &[Key::Named("H")];
pub(crate) const FILL_KEYS: Shortcut = &[Key::Named("F")];

/// The style a new annotation starts in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AnnotationStyle {
    pub color: CanvasAnnotationColor,
    pub text_size: CanvasTextSize,
    pub stroke_width: CanvasStrokeWidth,
    pub fill: CanvasRectangleFill,
}

/// The change the HUD is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StyleChange {
    Color(CanvasAnnotationColor),
    /// A mixed selection sizes labels and strokes together, so both scales show.
    Size {
        text: Option<CanvasTextSize>,
        stroke: Option<CanvasStrokeWidth>,
    },
    Arrowhead(bool),
    Fill(CanvasRectangleFill),
}

#[derive(Clone, Debug)]
pub(crate) struct StyleHud {
    change: StyleChange,
    /// The selection the change went to. The HUD closes once it changes;
    /// empty means the change went to the next annotation.
    selection: Vec<i64>,
    shown_at: f64,
}

/// How long the HUD stays before it fades.
const HUD_SECONDS: f64 = 1.1;
const DIGIT_KEYS: [egui::Key; 5] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
];

pub(super) fn handle_keys(ui: &egui::Ui, state: &mut CanvasState) {
    let ctx = ui.ctx();
    // A label being typed in takes every key as text.
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    let (digits, head, fill, now) = ctx.input(|input| {
        let mut digits = Vec::new();
        let mut head = false;
        let mut fill = false;
        for event in &input.events {
            let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            head |= *key == egui::Key::H && modifiers.is_none();
            fill |= *key == egui::Key::F && modifiers.is_none();
            // Shift turns a digit into a different character on every
            // layout, so digits go by where the key sits.
            let digit = physical_key.unwrap_or(*key);
            if let Some(index) = DIGIT_KEYS.iter().position(|candidate| *candidate == digit)
                && (modifiers.is_none() || modifiers.shift_only())
            {
                digits.push((index, modifiers.shift));
            }
        }
        (digits, head, fill, input.time)
    });

    // An armed draw mode is about the next annotation, so its style keys
    // leave the selection alone.
    if state.interaction.armed_annotation_kind.is_some() && (fill || !digits.is_empty()) {
        state.clear_selection();
    }
    if !state.interaction.selected_placement_ids.is_empty() {
        return;
    }
    let mut change = None;
    for (index, size) in digits {
        change = Some(if size {
            set_size(state, index)
        } else {
            set_color(state, CanvasAnnotationColor::ALL[index])
        });
    }
    if head {
        change = state
            .toggle_selected_arrowheads()
            .map(StyleChange::Arrowhead)
            .or(change);
    }
    if fill {
        change = step_fill(state).or(change);
    }
    if let Some(change) = change {
        state.interaction.style_hud = Some(StyleHud {
            change,
            selection: state.interaction.selected_annotation_ids.clone(),
            shown_at: now,
        });
    }
}

fn set_color(state: &mut CanvasState, color: CanvasAnnotationColor) -> StyleChange {
    state.apply_color_to_selected_annotations(color);
    state.interaction.next_annotation_style.color = color;
    StyleChange::Color(color)
}

/// Labels take the type size, and strokes and rectangle borders the weight,
/// at the same step. The next style follows whichever scales the selection
/// used, or both when nothing is selected.
fn set_size(state: &mut CanvasState, index: usize) -> StyleChange {
    let text_size = CanvasTextSize::ALL[index];
    let stroke_width = CanvasStrokeWidth::ALL[index];
    state.set_selected_annotation_sizes(text_size, stroke_width);
    let selected = state.selected_annotations();
    let is_label = |kind| kind == CanvasAnnotationKind::Textbox;
    let text = (selected.is_empty() || has_kind(&selected, is_label)).then_some(text_size);
    let stroke = (selected.is_empty() || has_kind(&selected, |kind| !is_label(kind)))
        .then_some(stroke_width);
    let next = &mut state.interaction.next_annotation_style;
    next.text_size = text.unwrap_or(next.text_size);
    next.stroke_width = stroke.unwrap_or(next.stroke_width);
    StyleChange::Size { text, stroke }
}

/// Steps the selected rectangles' fill, or the next rectangle's when nothing
/// is selected. A selection without a rectangle changes nothing.
fn step_fill(state: &mut CanvasState) -> Option<StyleChange> {
    let fill = if state.interaction.selected_annotation_ids.is_empty() {
        state.interaction.next_annotation_style.fill.next()
    } else {
        state.cycle_selected_rectangle_fills()?
    };
    state.interaction.next_annotation_style.fill = fill;
    Some(StyleChange::Fill(fill))
}

fn is_stroke(kind: CanvasAnnotationKind) -> bool {
    matches!(
        kind,
        CanvasAnnotationKind::Line | CanvasAnnotationKind::Arrow
    )
}

fn has_kind(
    annotations: &[&crate::data::CanvasAnnotation],
    kind: impl Fn(CanvasAnnotationKind) -> bool,
) -> bool {
    annotations.iter().any(|annotation| kind(annotation.kind))
}

/// Height of the strip the key row and HUD sit in.
const STRIP_HEIGHT: f32 = 56.0;
/// Half the width the key row and HUD can take.
const STRIP_HALF_WIDTH: f32 = 250.0;

/// The key row and the HUD, bottom-center of the canvas, or top-center when
/// the selection sits under the bottom strip.
pub(super) fn show_hud(ui: &egui::Ui, rect: egui::Rect, state: &mut CanvasState) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|input| input.time);
    let hud_open = state.interaction.style_hud.as_ref().is_some_and(|hud| {
        now - hud.shown_at < HUD_SECONDS
            && hud.selection == state.interaction.selected_annotation_ids
    });
    if let Some(hud) = &state.interaction.style_hud
        && hud_open
    {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            HUD_SECONDS - (now - hud.shown_at),
        ));
    }
    let hud_entrance =
        motion::overlay_entrance(&ctx, egui::Id::new("annotation_style_hud"), hud_open);
    if hud_entrance <= 0.0 {
        state.interaction.style_hud = None;
    }
    let hints = key_hints(state);
    // The key row waits for the HUD to leave so the two never cross-fade.
    let hints_entrance = motion::overlay_entrance(
        &ctx,
        egui::Id::new("annotation_style_keys"),
        hints.is_some() && state.interaction.style_hud.is_none(),
    );
    let bottom_strip = egui::Rect::from_x_y_ranges(
        rect.center().x - STRIP_HALF_WIDTH..=rect.center().x + STRIP_HALF_WIDTH,
        rect.bottom() - STRIP_HEIGHT..=rect.bottom(),
    );
    let (anchor, pivot) = if selection_screen_rect(state, rect)
        .is_some_and(|selection| selection.intersects(bottom_strip))
    {
        (
            egui::pos2(rect.center().x, rect.top() + theme::SPACE_4),
            egui::Align2::CENTER_TOP,
        )
    } else {
        (
            egui::pos2(rect.center().x, rect.bottom() - theme::SPACE_4),
            egui::Align2::CENTER_BOTTOM,
        )
    };

    if let Some(hints) = hints
        && hints_entrance > 0.0
    {
        egui::Area::new(egui::Id::new("annotation_style_keys"))
            .order(egui::Order::Foreground)
            .interactable(false)
            .pivot(pivot)
            .fixed_pos(anchor + egui::vec2(0.0, motion::settle_offset(hints_entrance)))
            .show(&ctx, |ui| {
                ui.set_opacity(hints_entrance);
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .corner_radius(theme::RADIUS_COMPACT)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = shortcut::HINT_GAP;
                            for (keys, label) in hints {
                                shortcut::hint(ui, keys, label);
                            }
                        });
                    });
            });
    }

    let Some(hud) = &state.interaction.style_hud else {
        return;
    };
    egui::Area::new(egui::Id::new("annotation_style_hud"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .pivot(pivot)
        .fixed_pos(anchor + egui::vec2(0.0, motion::settle_offset(hud_entrance)))
        .show(&ctx, |ui| {
            ui.set_opacity(hud_entrance);
            crate::ui::card::frame()
                .inner_margin(egui::Margin::symmetric(theme::SPACE_3 as i8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Sets the row height first so captions center on the cells.
                        ui.set_min_height(CELL.y + 2.0 * crate::ui::card::BED_INSET);
                        ui.spacing_mut().item_spacing.x = theme::SPACE_3;
                        paint_change(ui, hud);
                    });
                });
        });
}

fn selection_screen_rect(state: &CanvasState, rect: egui::Rect) -> Option<egui::Rect> {
    state
        .selected_annotations()
        .into_iter()
        .map(|annotation| super::interaction::annotation_screen_rect(state, rect, annotation))
        .reduce(egui::Rect::union)
}

type Hint = (&'static [Key], &'static str);

/// The keys that work on the armed draw mode or the current selection, or
/// none when there is neither.
fn key_hints(state: &CanvasState) -> Option<Vec<Hint>> {
    if let Some(kind) = state.interaction.armed_annotation_kind {
        let draw = match kind {
            CanvasAnnotationKind::Arrow => "Arrow",
            CanvasAnnotationKind::Line => "Line",
            CanvasAnnotationKind::Rectangle => "Rectangle",
            CanvasAnnotationKind::Textbox => return None,
        };
        let mut hints: Vec<Hint> = vec![
            (&[Key::Named("Drag")], draw),
            (COLOR_KEYS, "Color"),
            (SIZE_KEYS, "Size"),
        ];
        if kind == CanvasAnnotationKind::Rectangle {
            hints.push((FILL_KEYS, "Fill"));
        }
        hints.push((&[Key::Named("Esc")], "Cancel"));
        return Some(hints);
    }
    if !state.interaction.selected_placement_ids.is_empty()
        || state.interaction.selected_annotation_ids.is_empty()
    {
        return None;
    }
    if state.interaction.editing_text_annotation_id.is_some() {
        return Some(vec![
            (&[Key::Enter], "Done"),
            (&[Key::Shift, Key::Enter], "New line"),
        ]);
    }
    let selected = state.selected_annotations();
    let mut hints: Vec<Hint> = vec![(COLOR_KEYS, "Color"), (SIZE_KEYS, "Size")];
    if has_kind(&selected, is_stroke) {
        hints.push((HEAD_KEYS, "Head"));
    }
    if has_kind(&selected, |kind| kind == CanvasAnnotationKind::Rectangle) {
        hints.push((FILL_KEYS, "Fill"));
    }
    if state
        .selected_annotation()
        .is_some_and(|annotation| annotation.kind == CanvasAnnotationKind::Textbox)
    {
        hints.push((&[Key::Enter], "Edit"));
    }
    hints.push((&[Key::Delete], "Delete"));
    Some(hints)
}

const CELL: egui::Vec2 = egui::vec2(28.0, 28.0);

fn paint_change(ui: &mut egui::Ui, hud: &StyleHud) {
    let caption = |ui: &mut egui::Ui, name: &str| {
        let text = if hud.selection.is_empty() {
            format!("Next {name}")
        } else {
            name.to_owned()
        };
        ui.label(theme::editorial_caption(&text));
    };
    match hud.change {
        StyleChange::Color(lit) => {
            caption(ui, "color");
            for color in CanvasAnnotationColor::ALL {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, CELL.y), egui::Sense::hover());
                let center = rect.center();
                if color == lit {
                    ui.painter().circle_stroke(
                        center,
                        9.0,
                        egui::Stroke::new(1.5_f32, theme::TEXT),
                    );
                }
                ui.painter()
                    .circle_filled(center, 5.5, annotation_color(color));
            }
        }
        StyleChange::Size { text, stroke } => {
            if let Some(lit) = text {
                caption(ui, "text");
                scale(
                    ui,
                    &CanvasTextSize::ALL,
                    lit,
                    |painter, center, ink, size| {
                        let points = match size {
                            CanvasTextSize::S => 9.0,
                            CanvasTextSize::M => 11.5,
                            CanvasTextSize::L => 14.5,
                            CanvasTextSize::Xl => 18.0,
                            CanvasTextSize::Xxl => 22.0,
                        };
                        painter.text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            "A",
                            egui::FontId::proportional(points),
                            ink,
                        );
                    },
                );
            }
            if let Some(lit) = stroke {
                caption(ui, "stroke");
                scale(
                    ui,
                    &CanvasStrokeWidth::ALL,
                    lit,
                    |painter, center, ink, width| {
                        painter.line_segment(
                            [center - egui::vec2(7.0, 0.0), center + egui::vec2(7.0, 0.0)],
                            egui::Stroke::new(stroke_width_value(width), ink),
                        );
                    },
                );
            }
        }
        StyleChange::Arrowhead(lit) => {
            caption(ui, "head");
            scale(ui, &[false, true], lit, |painter, center, ink, head| {
                let from = center - egui::vec2(7.0, 0.0);
                let to = center + egui::vec2(7.0, 0.0);
                let stroke = egui::Stroke::new(1.5_f32, ink);
                painter.line_segment([from, to], stroke);
                if head {
                    painter.line_segment([to, to + egui::vec2(-4.0, -4.0)], stroke);
                    painter.line_segment([to, to + egui::vec2(-4.0, 4.0)], stroke);
                }
            });
        }
        StyleChange::Fill(lit) => {
            caption(ui, "fill");
            scale(
                ui,
                &CanvasRectangleFill::ALL,
                lit,
                |painter, center, ink, fill| {
                    let inside = match fill {
                        CanvasRectangleFill::None => egui::Color32::TRANSPARENT,
                        CanvasRectangleFill::Tint => ink.gamma_multiply(0.3),
                        CanvasRectangleFill::Solid => ink,
                    };
                    painter.rect(
                        egui::Rect::from_center_size(center, egui::vec2(16.0, 12.0)),
                        2.0,
                        inside,
                        egui::Stroke::new(1.5_f32, ink),
                        egui::StrokeKind::Inside,
                    );
                },
            );
        }
    }
}

/// A row of values on a bed, the lit one filled gold like a picked cell.
fn scale<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    values: &[T],
    lit: T,
    face: impl Fn(&egui::Painter, egui::Pos2, egui::Color32, T),
) {
    let inset = crate::ui::card::BED_INSET;
    let size = egui::vec2(
        values.len() as f32 * (CELL.x + inset) + inset,
        CELL.y + 2.0 * inset,
    );
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, crate::ui::card::BED_RADIUS, theme::SURFACE_PRESSED);
    for (index, value) in values.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            rect.min + egui::vec2(inset + index as f32 * (CELL.x + inset), inset),
            CELL,
        );
        let selected = *value == lit;
        if selected {
            painter.rect_filled(cell, theme::RADIUS_CONTROL, theme::ACCENT);
        }
        face(
            painter,
            cell.center(),
            crate::ui::card::ink(selected, true, !selected),
            *value,
        );
    }
}
