use eframe::egui;

fn fade_frame(frame: egui::Frame, entrance: f32) -> egui::Frame {
    let mut shadow = frame.shadow;
    shadow.color = shadow.color.gamma_multiply(entrance);
    frame
        .fill(frame.fill.gamma_multiply(entrance))
        .stroke(egui::Stroke::new(
            frame.stroke.width,
            frame.stroke.color.gamma_multiply(entrance),
        ))
        .shadow(shadow)
}

pub(super) fn destructive_confirmation_frame(ctx: &egui::Context, entrance: f32) -> egui::Frame {
    // Destructive dialogs keep the shell neutral. The destructive color belongs
    // on the irreversible action, not on the panel edge behind the sidebar.
    fade_frame(
        egui::Frame::window(&ctx.global_style())
            .fill(crate::ui::theme::SURFACE)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_PANEL))
            .inner_margin(egui::Margin::same(0)),
        entrance,
    )
}

pub(super) fn destructive_dialog_width() -> f32 {
    340.0
}

pub(super) fn semantic_search_prompt_width() -> f32 {
    380.0
}

pub(super) fn semantic_search_prompt_frame(ctx: &egui::Context, entrance: f32) -> egui::Frame {
    fade_frame(
        egui::Frame::window(&ctx.global_style())
            .fill(crate::ui::theme::SURFACE)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_PANEL))
            .inner_margin(egui::Margin::same(0)),
        entrance,
    )
}

pub(super) fn semantic_search_prompt_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(crate::ui::theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.add_space(crate::ui::theme::SPACE_3);
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Heading)
                .color(crate::ui::theme::TEXT),
        );
    });
    ui.add_space(crate::ui::theme::SPACE_2);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        crate::ui::theme::stroke_subtle(),
    );
}

pub(super) fn semantic_search_prompt_body(
    ui: &mut egui::Ui,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::ui::theme::SPACE_3 as i8,
            right: crate::ui::theme::SPACE_3 as i8,
            top: crate::ui::theme::SPACE_3 as i8,
            bottom: crate::ui::theme::SPACE_3 as i8,
        })
        .show(ui, |ui| {
            add_contents(ui);
        });
}

pub(super) fn destructive_dialog_header(ui: &mut egui::Ui, title: &str) {
    ui.add_space(crate::ui::theme::SPACE_3);
    ui.horizontal(|ui| {
        ui.add_space(crate::ui::theme::SPACE_3);
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Heading)
                .color(crate::ui::theme::TEXT),
        );
    });
    ui.add_space(crate::ui::theme::SPACE_2);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        crate::ui::theme::stroke_subtle(),
    );
}

pub(super) fn destructive_dialog_body(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::ui::theme::SPACE_3 as i8,
            right: crate::ui::theme::SPACE_3 as i8,
            top: crate::ui::theme::SPACE_3 as i8,
            bottom: crate::ui::theme::SPACE_3 as i8,
        })
        .show(ui, |ui| {
            add_contents(ui);
        });
}

pub(super) fn destructive_action_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(crate::ui::theme::TEXT),
    )
    .fill(crate::ui::theme::DESTRUCTIVE_FILL)
    .stroke(egui::Stroke::new(1.0, crate::ui::theme::DESTRUCTIVE_EDGE))
    .corner_radius(crate::ui::theme::RADIUS_INLINE)
}

pub(super) fn secondary_dialog_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(crate::ui::theme::TEXT),
    )
    .fill(crate::ui::theme::SURFACE_ELEVATED)
    .stroke(crate::ui::theme::stroke_subtle())
    .corner_radius(crate::ui::theme::RADIUS_INLINE)
}

pub(super) fn primary_dialog_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(crate::ui::theme::BACKGROUND),
    )
    .fill(crate::ui::theme::ACCENT)
    .stroke(egui::Stroke::new(1.0, crate::ui::theme::ACCENT))
    .corner_radius(crate::ui::theme::RADIUS_INLINE)
}
