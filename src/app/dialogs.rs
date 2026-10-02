//! The app's one dialog: a compact card that asks a question. The title is
//! the question, a muted line says what the action does, and the actions sit
//! bottom right, a quiet cancel beside one filled action.

use eframe::egui::{self, Align, Align2, FontId, Layout, Sense, UiBuilder, vec2};

use crate::ui::{card, motion, theme};

const WIDTH: f32 = 320.0;

pub(super) enum DialogAction<'a> {
    /// The gold button. Enter runs it.
    Primary(&'a str),
    /// The red button. Only a click runs it, so a stray Enter never
    /// destroys data.
    Destructive(&'a str),
}

pub(super) struct Dialog<'a> {
    /// Stable across frames while the title changes, like a board's name.
    pub id: &'static str,
    pub title: &'a str,
    pub detail: &'a str,
    pub cancel: &'a str,
    pub action: DialogAction<'a>,
}

pub(super) enum DialogChoice {
    Confirm,
    Cancel,
}

impl Dialog<'_> {
    /// Draws the dialog. Call it only on frames where it is open. Returns
    /// the choice made this frame by click or key: Escape cancels, Enter
    /// confirms a primary action.
    pub(super) fn show(&self, ctx: &egui::Context) -> Option<DialogChoice> {
        let id = egui::Id::new(("dialog", self.id));
        let entrance = motion::entrance_on_appear(ctx, id);
        let frame = card::frame();
        let mut shadow = frame.shadow;
        shadow.color = shadow.color.gamma_multiply(entrance);
        let frame = frame
            .fill(frame.fill.gamma_multiply(entrance))
            .shadow(shadow);
        let clicked = egui::Window::new(self.title)
            .id(id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(
                Align2::CENTER_CENTER,
                vec2(0.0, motion::settle_offset(entrance)),
            )
            .frame(frame)
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let inner = WIDTH - 2.0 * card::PADDING;
                ui.set_width(inner);
                text(ui, self.title, theme::TYPE_TITLE, theme::TEXT);
                ui.add_space(theme::SPACE_1);
                text(ui, self.detail, theme::TYPE_DETAIL, theme::TEXT_MUTED);
                ui.add_space(theme::SPACE_4);
                let (row, _) =
                    ui.allocate_exact_size(vec2(inner, theme::CONTROL_HEIGHT), Sense::hover());
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(row)
                        .layout(Layout::right_to_left(Align::Center)),
                    |ui| self.actions(ui),
                )
                .inner
            })
            .and_then(|window| window.inner)
            .flatten();
        clicked.or_else(|| {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    Some(DialogChoice::Cancel)
                } else if matches!(self.action, DialogAction::Primary(_))
                    && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                {
                    Some(DialogChoice::Confirm)
                } else {
                    None
                }
            })
        })
    }

    /// Lays the actions out right to left, so the filled one ends flush right.
    fn actions(&self, ui: &mut egui::Ui) -> Option<DialogChoice> {
        ui.spacing_mut().item_spacing.x = card::ITEM_SPACING.x;
        let confirm = match self.action {
            DialogAction::Primary(label) => card::button(ui, label, 0.0),
            DialogAction::Destructive(label) => card::destructive_button(ui, label, 0.0),
        };
        let cancel = quiet_button(ui, self.cancel);
        if confirm.clicked() {
            Some(DialogChoice::Confirm)
        } else if cancel.clicked() {
            Some(DialogChoice::Cancel)
        } else {
            None
        }
    }
}

fn text(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).wrap());
}

/// Cancel sized like the filled button beside it: control height and type,
/// muted ink that lifts on hover, no fill at rest.
fn quiet_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let font = FontId::proportional(theme::TYPE_CONTROL);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), theme::TEXT)
        .size()
        .x;
    let response = card::cell(
        ui,
        label,
        vec2(text_width + 2.0 * theme::SPACE_3, theme::CONTROL_HEIGHT),
        false,
        true,
        egui::Color32::TRANSPARENT,
    );
    let ink = if response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    ui.painter().text(
        response.rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        ink,
    );
    response
}
