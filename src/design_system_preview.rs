use eframe::egui;

pub(crate) fn show(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 760.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(
            screen_rect.center().x - panel_width * 0.5,
            screen_rect.top() + crate::theme::SPACE_8,
        ),
        egui::vec2(
            panel_width,
            screen_rect.height() - crate::theme::SPACE_8 * 2.0,
        ),
    );

    egui::Area::new(egui::Id::new("design_system_preview"))
        .order(egui::Order::Foreground)
        .fixed_pos(panel_rect.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(panel_rect.size());
            egui::Frame::new()
                .fill(crate::theme::SURFACE)
                .stroke(crate::theme::stroke_subtle())
                .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_PANEL))
                .inner_margin(egui::Margin::same(crate::theme::SPACE_4 as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(crate::theme::SPACE_4, crate::theme::SPACE_3);
                    ui.label(
                        egui::RichText::new("SnappyRef design system")
                            .text_style(crate::theme::display_text_style())
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Dense, quiet desktop UI for arranging visual references without adding chrome to the canvas.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                    );

                    ui.add_space(crate::theme::SPACE_2);
                    ui.columns(2, |columns| {
                        color_tokens(&mut columns[0]);
                        type_scale(&mut columns[1]);
                    });
                    ui.add_space(crate::theme::SPACE_2);
                    controls_and_surfaces(ui);
                });
        });
}

fn color_tokens(ui: &mut egui::Ui) {
    section_label(ui, "Colors");
    for (name, color) in [
        ("Background", crate::theme::BACKGROUND),
        ("Surface", crate::theme::SURFACE),
        ("Elevated", crate::theme::SURFACE_ELEVATED),
        ("Text", crate::theme::TEXT),
        ("Muted", crate::theme::TEXT_MUTED),
        ("Blue", crate::theme::ACCENT_BLUE),
        ("Green", crate::theme::ACCENT_GREEN),
        ("Orange", crate::theme::ACCENT_ORANGE),
    ] {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(34.0, 18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, crate::theme::RADIUS_CONTROL, color);
            ui.painter().rect_stroke(
                rect,
                crate::theme::RADIUS_CONTROL,
                crate::theme::stroke_subtle(),
                egui::StrokeKind::Outside,
            );
            ui.label(egui::RichText::new(name).color(crate::theme::TEXT));
        });
    }
}

fn type_scale(ui: &mut egui::Ui) {
    section_label(ui, "Type");
    for (label, style) in [
        ("Display", crate::theme::display_text_style()),
        ("Body", egui::TextStyle::Body),
        ("Small", egui::TextStyle::Small),
    ] {
        ui.label(
            egui::RichText::new(label)
                .text_style(style)
                .color(crate::theme::TEXT),
        );
    }
}

fn controls_and_surfaces(ui: &mut egui::Ui) {
    section_label(ui, "Controls");
    ui.horizontal(|ui| {
        control_sample(ui, "Compact", crate::theme::RADIUS_COMPACT, true);
        control_sample(ui, "Control", crate::theme::RADIUS_CONTROL, false);
        control_sample(ui, "Panel", crate::theme::RADIUS_PANEL, false);
    });

    ui.add_space(crate::theme::SPACE_2);
    ui.horizontal(|ui| {
        surface_sample(ui, "Surface", crate::theme::SURFACE);
        surface_sample(ui, "Elevated", crate::theme::SURFACE_ELEVATED);
        surface_sample(ui, "Palette", crate::theme::PALETTE_SURFACE);
    });
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_SUBTLE)
            .strong(),
    );
}

fn control_sample(ui: &mut egui::Ui, label: &str, radius: u8, selected: bool) {
    let size = egui::vec2(104.0, 30.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let fill = if selected {
        crate::theme::SURFACE_ELEVATED
    } else {
        crate::theme::SURFACE
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(
        rect,
        radius,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Outside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
        crate::theme::TEXT,
    );
}

fn surface_sample(ui: &mut egui::Ui, label: &str, fill: egui::Color32) {
    let size = egui::vec2(150.0, 72.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, crate::theme::RADIUS_PANEL, fill);
    ui.painter().rect_stroke(
        rect,
        crate::theme::RADIUS_PANEL,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Outside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(12.0, egui::FontFamily::Proportional),
        crate::theme::TEXT_MUTED,
    );
}
