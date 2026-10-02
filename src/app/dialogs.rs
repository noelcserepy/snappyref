use eframe::egui;

use crate::ui::{card, theme};

/// A dialog is a floating card that fades in with its overlay entrance.
/// Destructive dialogs keep the shell neutral: the red belongs on the
/// irreversible action, not on the card behind it.
pub(super) fn dialog_frame(entrance: f32) -> egui::Frame {
    let frame = card::frame();
    let mut shadow = frame.shadow;
    shadow.color = shadow.color.gamma_multiply(entrance);
    frame
        .fill(frame.fill.gamma_multiply(entrance))
        .shadow(shadow)
}

pub(super) fn destructive_dialog_width() -> f32 {
    340.0
}

pub(super) fn semantic_search_prompt_width() -> f32 {
    380.0
}

pub(super) fn dialog_title(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .size(theme::TYPE_TITLE)
            .color(theme::TEXT),
    );
}

/// The question a dialog asks, under its title.
pub(super) fn dialog_lead(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::TYPE_CONTROL)
            .color(theme::TEXT),
    );
}

/// What the dialog's action does, wrapped under the lead.
pub(super) fn dialog_detail(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(theme::TYPE_DETAIL)
                .color(theme::TEXT_MUTED),
        )
        .wrap(),
    );
}

pub(super) enum DialogChoice {
    Confirm,
    Cancel,
}

/// The dialog's closing row: the action on the right, Cancel quiet beside
/// it. `action` draws the action button and returns its response.
pub(super) fn dialog_actions(
    ui: &mut egui::Ui,
    cancel: &str,
    action: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> Option<DialogChoice> {
    ui.add_space(theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if action(ui).clicked() {
                Some(DialogChoice::Confirm)
            } else if card::action(ui, cancel, true).clicked() {
                Some(DialogChoice::Cancel)
            } else {
                None
            }
        })
        .inner
    })
    .inner
}
