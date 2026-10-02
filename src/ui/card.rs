//! The Group card's vocabulary, shared by every floating card and its
//! controls: the card frame, section captions, separators, inset wells,
//! fields, segmented beds, navigation tabs, switches, the primary and
//! destructive buttons, quiet text actions, progress tracks, status lines and panels, and
//! tooltips.
//!
//! Inside a card, controls are [`theme::CONTROL_HEIGHT`] tall and sit
//! [`ITEM_SPACING`] apart; [`section`] adds 6 more above each caption, so
//! sections read 14 apart.

use eframe::egui::emath::GuiRounding as _;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};

use super::shortcut::{self, Shortcut};
use super::{motion, theme};

/// Card padding.
pub(crate) const PADDING: f32 = theme::SPACE_3;
/// Controls sit 6 apart on a line, and lines 8 apart.
pub(crate) const ITEM_SPACING: Vec2 = egui::vec2(6.0, 8.0);
/// A [`status`] line's dot, and where its label starts.
const STATUS_DOT: f32 = 6.0;
const STATUS_INSET: f32 = STATUS_DOT + theme::SPACE_2;
/// A bed's radius keeps its 8px cells concentric inside the 2px margin.
pub(crate) const BED_RADIUS: u8 = 10;
/// A bed's margin around its cells, and the gap between them.
pub(crate) const BED_INSET: f32 = 2.0;

/// The floating card: surface fill, card radius, deep soft shadow, and
/// padding. Menus tighten the padding and radius together; see `RADIUS_PANEL`.
pub(crate) fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(theme::RADIUS_CARD)
        .inner_margin(PADDING)
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(180),
        })
}

/// A section caption with the extra space that separates it from the
/// section above.
pub(crate) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0 - ITEM_SPACING.y);
    ui.label(theme::editorial_caption(title));
}

/// A hairline rule between groups that need no caption, with 4 clear above
/// and below.
pub(crate) fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0 + 2.0 * theme::SPACE_1),
        Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range().shrink(6.0),
        rect.center().y,
        Stroke::new(1.0_f32, theme::SURFACE_RAISED),
    );
}

/// An inset well `width` wide: the field and switch background. Contents
/// lay out left to right, vertically centered, inside 10px side padding.
pub(crate) fn well<R>(
    ui: &mut egui::Ui,
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE_ELEVATED)
        .corner_radius(theme::RADIUS_CONTROL)
        .inner_margin(egui::Margin::symmetric(10, 0))
        .show(ui, |ui| {
            let inner = width - 20.0;
            ui.allocate_ui_with_layout(
                egui::vec2(inner, theme::CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(inner);
                    contents(ui)
                },
            )
            .inner
        })
        .inner
}

/// A segmented control's bed. Fill it with [`cell`]s on `SURFACE_PRESSED`.
pub(crate) fn bed(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE_PRESSED)
        .corner_radius(BED_RADIUS)
        .inner_margin(BED_INSET)
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(0.0, theme::CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = BED_INSET;
                    contents(ui);
                },
            );
        });
}

/// A named, clickable cell that reports its selected state. Selected cells
/// fill gold; others show `fill` at rest and lift on hover. Paint the face
/// with [`ink`].
pub(crate) fn cell(
    ui: &mut egui::Ui,
    name: &str,
    size: Vec2,
    selected: bool,
    enabled: bool,
    fill: Color32,
) -> Response {
    let response = hit(ui, name, size, selected, enabled);
    let color = if selected {
        theme::ACCENT
    } else {
        lift(&response, fill)
    };
    ui.painter()
        .rect_filled(response.rect, theme::RADIUS_CONTROL, color);
    focus_ring(ui, &response);
    response
}

/// Navigation tabs `size` big: a bed with one raised fill that slides under
/// the open tab. Gold stays with [`cell`]s, the values a user picked, so a
/// page of choices never reads as several selections. `face` paints tab `i`
/// into its rect in the given ink. Returns the tab clicked this frame.
pub(crate) fn tabs(
    ui: &mut egui::Ui,
    size: Vec2,
    names: &[&str],
    open: usize,
    face: impl Fn(&egui::Painter, usize, Rect, Color32),
) -> Option<usize> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, BED_RADIUS, theme::SURFACE_PRESSED);
    let inner = rect.shrink(BED_INSET);
    let count = names.len() as f32;
    let width = (inner.width() - BED_INSET * (count - 1.0)) / count;
    let tab = |i: usize| {
        Rect::from_min_size(
            egui::pos2(inner.left() + i as f32 * (width + BED_INSET), inner.top()),
            egui::vec2(width, inner.height()),
        )
    };
    // Relative to the bed, so the fill rides along when the card itself moves.
    let x = motion::slide_indicator(
        ui.ctx(),
        response.id.with("open"),
        tab(open).left() - inner.left(),
    );
    ui.painter().rect_filled(
        tab(open).translate(egui::vec2(x + inner.left() - tab(open).left(), 0.0)),
        theme::RADIUS_CONTROL,
        theme::SURFACE_ELEVATED,
    );
    let mut clicked = None;
    for (i, name) in names.iter().enumerate() {
        let r = tab(i);
        let response = ui.interact(r, response.id.with(i), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), i == open, *name)
        });
        if i != open && response.hovered() {
            ui.painter().rect_filled(
                r,
                theme::RADIUS_CONTROL,
                motion::mix(theme::SURFACE_PRESSED, theme::SURFACE_ELEVATED, 0.5),
            );
        }
        focus_ring(ui, &response);
        let ink = if i == open || response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        };
        face(ui.painter(), i, r, ink);
        if response.clicked() && i != open {
            clicked = Some(i);
        }
    }
    clicked
}

/// Ink for a cell's face: dark on gold, muted or full text otherwise, and
/// faded while disabled.
pub(crate) fn ink(selected: bool, enabled: bool, muted: bool) -> Color32 {
    let color = if selected {
        theme::BACKGROUND
    } else if muted {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    };
    color.gamma_multiply(fade(enabled))
}

/// A named on/off row: icon and name on the left, track on the right, and
/// the shortcut that toggles it, if any, just before the track.
/// `icon` paints the 14px icon centered at the given point.
pub(crate) fn switch(
    ui: &mut egui::Ui,
    name: &str,
    width: f32,
    on: bool,
    enabled: bool,
    keys: Option<Shortcut>,
    icon: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> Response {
    let response = hit(
        ui,
        name,
        egui::vec2(width, theme::CONTROL_HEIGHT),
        on,
        enabled,
    );
    let r = response.rect;
    ui.painter()
        .rect_filled(r, theme::RADIUS_CONTROL, theme::SURFACE_ELEVATED);
    focus_ring(ui, &response);
    let p = ui.painter();
    let fade = fade(enabled);
    icon(
        p,
        r.left_center() + egui::vec2(17.0, 0.0),
        theme::TEXT_MUTED.gamma_multiply(fade),
    );
    p.text(
        r.left_center() + egui::vec2(30.0, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(theme::TYPE_CONTROL),
        theme::TEXT.gamma_multiply(fade),
    );
    if let Some(keys) = keys {
        let mut faded = p.clone();
        faded.multiply_opacity(fade);
        shortcut::paint(
            &faded,
            r.right_center() - egui::vec2(48.0, 0.0),
            Align2::RIGHT_CENTER,
            keys,
            theme::TEXT_MUTED,
        );
    }
    let track = egui::Rect::from_center_size(
        r.right_center() - egui::vec2(25.0, 0.0),
        egui::vec2(30.0, 18.0),
    );
    let track_fill = if on {
        theme::ACCENT
    } else {
        theme::SURFACE_RAISED
    };
    p.rect_filled(track, 9, track_fill.gamma_multiply(fade));
    let knob = if on {
        theme::BACKGROUND
    } else if response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    p.circle_filled(
        track.center() + egui::vec2(if on { 6.0 } else { -6.0 }, 0.0),
        7.0,
        knob.gamma_multiply(fade),
    );
    response
}

/// The card's primary action: a gold button with dark ink, at least
/// `min_width` wide and wider when its label needs it. A card has at most one.
pub(crate) fn button(ui: &mut egui::Ui, label: &str, min_width: f32) -> Response {
    filled_button(ui, label, min_width, theme::ACCENT, theme::BACKGROUND)
}

/// [`button`] for an action that destroys data, like Delete board: red
/// fill, light ink. It replaces the primary button, never sits beside one.
pub(crate) fn destructive_button(ui: &mut egui::Ui, label: &str, min_width: f32) -> Response {
    filled_button(ui, label, min_width, theme::DESTRUCTIVE, theme::TEXT)
}

fn filled_button(
    ui: &mut egui::Ui,
    label: &str,
    min_width: f32,
    fill: Color32,
    ink: Color32,
) -> Response {
    let font = FontId::proportional(theme::TYPE_CONTROL);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), ink)
        .size()
        .x;
    let width = min_width.max(text_width + 2.0 * theme::SPACE_3);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::CONTROL_HEIGHT), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if response.is_pointer_button_down_on() {
        motion::mix(fill, theme::BACKGROUND, 0.15)
    } else if response.hovered() {
        motion::mix(fill, theme::TEXT, 0.3)
    } else {
        fill
    };
    let p = ui.painter();
    p.rect_filled(rect, theme::RADIUS_CONTROL, fill);
    p.text(rect.center(), Align2::CENTER_CENTER, label, font, ink);
    response
}

/// A quiet text action, like the Group card's Ungroup: muted text that
/// lifts on hover.
pub(crate) fn action(ui: &mut egui::Ui, label: &str, enabled: bool) -> Response {
    let font = FontId::proportional(theme::TYPE_DETAIL);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT)
        .size()
        .x;
    let response = cell(
        ui,
        label,
        egui::vec2(text_width + 16.0, 26.0),
        false,
        enabled,
        Color32::TRANSPARENT,
    );
    ui.painter().text(
        response.rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        ink(false, enabled, true),
    );
    response
}

/// A determinate progress track `width` wide: gold fill on the raised tone,
/// so the track reads on the card and on a [`status_panel`]'s bed alike.
pub(crate) fn progress(ui: &mut egui::Ui, width: f32, fraction: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 4.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2, theme::SURFACE_RAISED);
    let mut filled = rect;
    filled.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    p.rect_filled(filled, 2, theme::ACCENT);
    response
}

/// A status dot in `color` and its label, on the hint label's type. The
/// label centers on the row like every other label, so it shares their
/// baseline; the dot then centers on the label's x-height, where a typeset
/// bullet sits.
pub(crate) fn status(ui: &mut egui::Ui, color: Color32, label: &str) -> Response {
    let font = FontId::proportional(theme::TYPE_DETAIL);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT_MUTED);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(STATUS_INSET + galley.size().x, 18.0),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    let p = ui.painter();
    // Text draws on whole pixels, so the dot measures from where it lands.
    let text = egui::pos2(
        rect.left() + STATUS_INSET,
        rect.center().y - galley.size().y * 0.5,
    )
    .round_to_pixels(ui.pixels_per_point());
    p.circle_filled(
        egui::pos2(
            rect.left() + STATUS_DOT * 0.5,
            text.y + x_height_center(p, font),
        ),
        STATUS_DOT * 0.5,
        color,
    );
    p.galley(text, galley, theme::TEXT_MUTED);
    response
}

/// How far below the top of a line of `font` the middle of its x-height
/// sits, measured from the ink of an "x".
fn x_height_center(painter: &egui::Painter, font: FontId) -> f32 {
    let galley = painter.layout_no_wrap("x".to_owned(), font, theme::TEXT);
    galley
        .rows
        .first()
        .and_then(|row| {
            let glyph = row.row.glyphs.first()?;
            Some(row.pos.y + glyph.pos.y + glyph.uv_rect.offset.y + glyph.uv_rect.size.y * 0.5)
        })
        .unwrap_or(galley.size().y * 0.5)
}

/// What a [`status_panel`] shows.
pub(crate) struct StatusPanel<'a> {
    pub title: &'a str,
    pub action: &'a str,
    /// The action is the step the user should take next, so it draws as the
    /// gold [`button`].
    pub primary: bool,
    pub enabled: bool,
    /// The [`status`] dot: green when ready, gold while something wants the
    /// user or is under way, red when it failed.
    pub dot: Color32,
    pub line: &'a str,
    /// Download progress from 0 to 1, drawn as a [`progress`] track under
    /// the line.
    pub progress: Option<f32>,
    pub detail: &'a str,
}

/// A thing the app installs or keeps current: icon, title, and its next
/// action on top, then a [`status`] line, optional [`progress`], and a
/// wrapped detail, all indented under the title. The panel sits on the bed
/// tone so a quiet action still lifts on hover. Returns the action's response.
pub(crate) fn status_panel(
    ui: &mut egui::Ui,
    width: f32,
    panel: &StatusPanel,
    icon: impl FnOnce(&egui::Painter, Pos2, Color32),
) -> Response {
    // The title sits 6 past the 14px icon; status text lines up under it.
    const INDENT: f32 = 20.0;
    egui::Frame::new()
        .fill(theme::SURFACE_PRESSED)
        .corner_radius(BED_RADIUS)
        .inner_margin(egui::Margin {
            left: 10,
            right: 6,
            top: 6,
            bottom: 10,
        })
        .show(ui, |ui| {
            let inner = width - 16.0;
            ui.set_width(inner);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let (head, title) =
                ui.allocate_exact_size(egui::vec2(inner, theme::CONTROL_HEIGHT), Sense::hover());
            title.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, panel.title)
            });
            icon(
                ui.painter(),
                head.left_center() + egui::vec2(7.0, 0.0),
                theme::TEXT_MUTED,
            );
            ui.painter().text(
                head.left_center() + egui::vec2(INDENT, 0.0),
                Align2::LEFT_CENTER,
                panel.title,
                FontId::proportional(theme::TYPE_CONTROL),
                theme::TEXT,
            );
            let action = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(head)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        if panel.primary && panel.enabled {
                            button(ui, panel.action, 0.0)
                        } else {
                            action(ui, panel.action, panel.enabled)
                        }
                    },
                )
                .inner;
            let text = inner - INDENT - 4.0;
            indented(ui, 18.0, INDENT - STATUS_INSET, |ui| {
                status(ui, panel.dot, panel.line);
            });
            if let Some(fraction) = panel.progress {
                ui.add_space(6.0);
                indented(ui, 4.0, INDENT, |ui| {
                    progress(ui, text, fraction);
                });
            }
            ui.add_space(4.0);
            let galley = ui.painter().layout(
                panel.detail.to_owned(),
                FontId::proportional(theme::TYPE_DETAIL),
                theme::TEXT_SUBTLE,
                text,
            );
            let (detail, _) =
                ui.allocate_exact_size(egui::vec2(inner, galley.size().y), Sense::hover());
            ui.painter().galley(
                detail.min + egui::vec2(INDENT, 0.0),
                galley,
                theme::TEXT_SUBTLE,
            );
            action
        })
        .inner
}

/// A full-width row `height` tall whose contents start `indent` in.
fn indented(ui: &mut egui::Ui, height: f32, indent: f32, add: impl FnOnce(&mut egui::Ui)) {
    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row.with_min_x(row.left() + indent))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        add,
    );
}

/// A frameless text field `width` by `height`, keyed by `name` alone so
/// focus and drafts survive the field moving. Enter, a press elsewhere, or
/// losing focus submits; Escape cancels. Returns the submitted text when it
/// differs from `value`, and whether the field holds focus.
#[allow(clippy::too_many_arguments)]
pub(crate) fn field(
    ui: &mut egui::Ui,
    name: &str,
    value: &str,
    width: f32,
    height: f32,
    enabled: bool,
    size: f32,
    align: egui::Align,
) -> (Option<String>, bool) {
    let id = field_id(name);
    let font = FontId::proportional(size);
    let key = id.with("draft");
    let mut draft = ui
        .ctx()
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| value.into());
    let was_focused = ui.memory(|m| m.has_focus(id));
    let output = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                egui::vec2(width, height),
                egui::TextEdit::singleline(&mut draft)
                    .id(id)
                    .desired_width(width)
                    .font(font)
                    .frame(egui::Frame::NONE)
                    .min_size(egui::vec2(width, height))
                    .horizontal_align(align)
                    .vertical_align(egui::Align::Center),
            )
        })
        .inner;
    output.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, name));
    let escape =
        (was_focused || output.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = output.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let outside = output.has_focus()
        && ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !output.rect.contains(p))
        });
    let submit = output.lost_focus() || enter || outside;
    if escape {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    if escape || enter || outside {
        output.surrender_focus();
    }
    if escape || submit || !output.has_focus() {
        ui.ctx().data_mut(|d| d.remove::<String>(key));
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(key, draft.clone()));
    }
    (
        (submit && !escape && draft != value).then_some(draft),
        output.has_focus() && !escape && !enter,
    )
}

/// Moves keyboard focus to the [`field`] called `name`, for fields that
/// appear on demand.
pub(crate) fn focus_field(ctx: &egui::Context, name: &str) {
    ctx.memory_mut(|m| m.request_focus(field_id(name)));
}

fn field_id(name: &str) -> egui::Id {
    egui::Id::new(("card-field", name))
}

/// A field's drag handle: dragging the icon scrubs `value` one unit per
/// pixel, never below `min`. Returns the scrubbed value while it differs
/// from `value`, and whether a drag is under way. Each frame adds only
/// movement not yet applied to the current value, so a draft that the same
/// press commits becomes the base.
pub(crate) fn scrub(
    ui: &mut egui::Ui,
    (name, tooltip): (&str, &str),
    value: f32,
    min: f32,
    enabled: bool,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
) -> (Option<f32>, bool) {
    let height = ui.available_height();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, height), Sense::hover());
    let response = ui.interact(
        rect.expand2(egui::vec2(6.0, 0.0)),
        egui::Id::new(("card-scrub", name)),
        if enabled {
            Sense::drag()
        } else {
            Sense::hover()
        },
    );
    let color = if response.dragged() {
        theme::ACCENT
    } else if response.hovered() && enabled {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    paint(ui.painter(), rect, color);
    let applied = response.id.with("applied");
    if response.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(applied, 0.0_f32));
    }
    let scrubbed = response.dragged().then(|| {
        let dx = ui.input(|i| {
            i.pointer
                .interact_pos()
                .zip(i.pointer.press_origin())
                .map_or(0.0, |(p, o)| p.x - o.x)
        });
        let taken = ui.ctx().data(|d| d.get_temp(applied)).unwrap_or(0.0_f32);
        let n = (value + dx - taken).round().max(min);
        ui.ctx()
            .data_mut(|d| d.insert_temp(applied, taken + n - value));
        n
    });
    let dragging = response.dragged();
    if enabled {
        tip(
            response.on_hover_and_drag_cursor(egui::CursorIcon::ResizeHorizontal),
            &format!("{tooltip} · drag to change"),
        );
    }
    (scrubbed.filter(|n| *n != value), dragging)
}

/// Card tooltips sit above their control on the tooltip tone, because the
/// global popup fill matches the card and would vanish against it.
pub(crate) fn tip(response: Response, text: &str) -> Response {
    let mut tooltip = if response.enabled() {
        egui::Tooltip::for_enabled(&response)
    } else {
        egui::Tooltip::for_disabled(&response)
    };
    tooltip.popup = tooltip.popup.align(egui::RectAlign::TOP).gap(8.0).frame(
        egui::Frame::new()
            .fill(theme::TOOLTIP)
            .corner_radius(theme::RADIUS_COMPACT)
            .inner_margin(egui::Margin::symmetric(8, 4)),
    );
    tooltip.show(|ui| {
        ui.label(
            egui::RichText::new(text)
                .size(theme::TYPE_DETAIL)
                .color(theme::TEXT),
        );
    });
    response
}

fn hit(ui: &mut egui::Ui, name: &str, size: Vec2, selected: bool, enabled: bool) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (_, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, name)
        });
        response
    })
    .inner
}

/// A control's fill under the pointer: pressed while held, elevated on
/// hover, `fill` at rest.
fn lift(response: &Response, fill: Color32) -> Color32 {
    if response.is_pointer_button_down_on() {
        theme::SURFACE_PRESSED
    } else if response.hovered() {
        theme::SURFACE_ELEVATED
    } else {
        fill
    }
}

fn focus_ring(ui: &egui::Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            theme::RADIUS_CONTROL,
            Stroke::new(1.0_f32, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}

/// Disabled controls draw at this opacity.
pub(crate) fn fade(enabled: bool) -> f32 {
    if enabled { 1.0 } else { 0.4 }
}
