use eframe::egui;

const PREVIEW_WIDTH: f32 = 860.0;
const ROW_HEIGHT: f32 = 42.0;

pub(crate) fn show(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = PREVIEW_WIDTH.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
    let panel_rect = egui::Rect::from_min_size(
        egui::pos2(
            screen_rect.center().x - panel_width * 0.5,
            screen_rect.top() + crate::theme::SPACE_6,
        ),
        egui::vec2(
            panel_width,
            screen_rect.height() - crate::theme::SPACE_6 * 2.0,
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
                    ui.set_max_width(panel_rect.width() - crate::theme::SPACE_4 * 2.0);
                    header(ui);
                    ui.add_space(crate::theme::SPACE_2);
                    token_strip(ui);
                    ui.add_space(crate::theme::SPACE_3);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing =
                                egui::vec2(crate::theme::SPACE_4, crate::theme::SPACE_3);
                            ui.columns(2, |columns| {
                                left_column(&mut columns[0]);
                                right_column(&mut columns[1]);
                            });
                        });
                });
        });
}

fn header(ui: &mut egui::Ui) {
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
}

fn left_column(ui: &mut egui::Ui) {
    board_rows(ui);
    ui.add_space(crate::theme::SPACE_2);
    quiet_settings(ui);
}

fn right_column(ui: &mut egui::Ui) {
    palette_overlay(ui);
    ui.add_space(crate::theme::SPACE_2);
    canvas_toolbar(ui);
    ui.add_space(crate::theme::SPACE_2);
    dialog_buttons(ui);
    ui.add_space(crate::theme::SPACE_2);
    radius_samples(ui);
}

fn token_strip(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::QUIET_INSET_FILL)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_3 as i8,
            right: crate::theme::SPACE_3 as i8,
            top: crate::theme::SPACE_2 as i8,
            bottom: crate::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing =
                    egui::vec2(crate::theme::SPACE_3, crate::theme::SPACE_1);
                for (name, color) in [
                    ("Background", crate::theme::BACKGROUND),
                    ("Surface", crate::theme::SURFACE),
                    ("Elevated", crate::theme::SURFACE_ELEVATED),
                    ("Palette", crate::theme::PALETTE_SURFACE),
                    ("Accent", crate::theme::ACCENT),
                    ("Blue guide", crate::theme::ACCENT_BLUE),
                    ("Green guide", crate::theme::ACCENT_GREEN),
                    ("Indexed", crate::theme::INDEXED_DOT),
                    ("Destructive", crate::theme::DESTRUCTIVE),
                ] {
                    token_chip(ui, name, color);
                }
                ui.label(
                    egui::RichText::new("Display / Body / Small")
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_MUTED),
                );
            });
        });
}

fn token_chip(ui: &mut egui::Ui, name: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_1;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 12.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, crate::theme::RADIUS_INLINE, color);
        ui.painter().rect_stroke(
            rect,
            crate::theme::RADIUS_INLINE,
            crate::theme::stroke_subtle(),
            egui::StrokeKind::Outside,
        );
        ui.label(
            egui::RichText::new(name)
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
        );
    });
}

fn board_rows(ui: &mut egui::Ui) {
    section_label(ui, "Sidebar board rows");
    board_row(
        ui,
        "Moodboard",
        "24 images",
        Some("⌘1"),
        BoardRowState::Active,
    );
    ui.add_space(crate::theme::SPACE_1);
    board_row(
        ui,
        "Lighting refs",
        "8 images",
        Some("⌘2"),
        BoardRowState::Idle,
    );
    ui.add_space(crate::theme::SPACE_1);
    board_row(ui, "Materials", "13 images", None, BoardRowState::DeleteHot);
    ui.add_space(crate::theme::SPACE_3);
    new_board_row(ui);
}

#[derive(Clone, Copy)]
enum BoardRowState {
    Active,
    Idle,
    DeleteHot,
}

fn board_row(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    shortcut: Option<&str>,
    state: BoardRowState,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let hot = matches!(state, BoardRowState::DeleteHot);
    let active = matches!(state, BoardRowState::Active);
    let fill = if active || hot {
        crate::theme::SURFACE_ELEVATED
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        fill,
        if active {
            crate::theme::stroke_subtle()
        } else {
            egui::Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    if active {
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + crate::theme::SPACE_2),
            egui::pos2(rect.left() + 2.0, rect.bottom() - crate::theme::SPACE_2),
        );
        ui.painter().rect_filled(
            rail_rect,
            egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
            crate::theme::ACCENT,
        );
    }

    let swatch_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + crate::theme::SPACE_3, rect.center().y - 10.0),
        egui::vec2(20.0, 20.0),
    );
    ui.painter().rect_filled(
        swatch_rect,
        egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
        egui::Color32::from_white_alpha(if active { 38 } else { 18 }),
    );

    let text_x = swatch_rect.right() + crate::theme::SPACE_2;
    ui.painter().text(
        egui::pos2(text_x, rect.top() + 8.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::TextStyle::Body.resolve(ui.style()),
        crate::theme::TEXT,
    );
    ui.painter().text(
        egui::pos2(text_x, rect.top() + 24.0),
        egui::Align2::LEFT_TOP,
        detail,
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT_MUTED,
    );

    let trailing = if hot {
        "Delete"
    } else {
        shortcut.unwrap_or("")
    };
    let color = if hot {
        crate::theme::DESTRUCTIVE
    } else {
        crate::theme::TEXT_SUBTLE
    };
    ui.painter().text(
        egui::pos2(rect.right() - crate::theme::SPACE_3, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        trailing,
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
}

fn new_board_row(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        egui::Color32::from_white_alpha(4),
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+  New board",
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT_SUBTLE,
    );
}

fn quiet_settings(ui: &mut egui::Ui) {
    section_label(ui, "Quiet settings");
    subcard(ui, |ui| {
        setting_row(
            ui,
            "Pointer mode",
            "Scroll wheel zooms. Hold Alt to pan.",
            |ui| {
                inline_choice(ui, "Mouse", true);
                inline_choice(ui, "Trackpad", false);
            },
        );
        divider(ui);
        setting_row(
            ui,
            "Always on top",
            "Keep SnappyRef above normal windows.",
            |ui| {
                inline_choice(ui, "On", true);
            },
        );
        divider(ui);
        setting_row(ui, "Image gap", "Gap used when placing new images.", |ui| {
            number_field_sample(ui, "24", "units");
        });
    });
}

fn setting_row(ui: &mut egui::Ui, title: &str, detail: &str, controls: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(crate::theme::SPACE_2);
    ui.label(
        egui::RichText::new(title)
            .text_style(egui::TextStyle::Body)
            .color(crate::theme::TEXT),
    );
    ui.label(
        egui::RichText::new(detail)
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_SUBTLE),
    );
    ui.add_space(crate::theme::SPACE_2);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
        controls(ui);
    });
    ui.add_space(crate::theme::SPACE_2);
}

fn inline_choice(ui: &mut egui::Ui, label: &str, active: bool) {
    let text_color = if active {
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let button = egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Button)
            .color(text_color),
    )
    .fill(if active {
        crate::theme::QUIET_INLINE_ACTIVE_FILL
    } else {
        egui::Color32::TRANSPARENT
    })
    .stroke(if active {
        crate::theme::stroke_subtle()
    } else {
        egui::Stroke::NONE
    })
    .corner_radius(crate::theme::RADIUS_INLINE)
    .min_size(egui::vec2(
        crate::theme::QUIET_INLINE_MIN_WIDTH,
        crate::theme::QUIET_INLINE_HEIGHT,
    ));
    ui.add(button);
}

fn number_field_sample(ui: &mut egui::Ui, value: &str, suffix: &str) {
    egui::Frame::new()
        .fill(crate::theme::QUIET_INLINE_ACTIVE_FILL)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_INLINE))
        .inner_margin(egui::Margin {
            left: crate::theme::QUIET_INLINE_PAD_X as i8,
            right: crate::theme::QUIET_INLINE_PAD_X as i8,
            top: crate::theme::SPACE_1 as i8,
            bottom: crate::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
                ui.label(
                    egui::RichText::new(value)
                        .text_style(egui::TextStyle::Button)
                        .color(crate::theme::TEXT),
                );
                ui.label(
                    egui::RichText::new(suffix)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_MUTED),
                );
            });
        });
}

fn palette_overlay(ui: &mut egui::Ui) {
    section_label(ui, "Search palette");
    egui::Frame::new()
        .fill(crate::theme::PALETTE_SURFACE)
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(33)))
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_PANEL))
        .inner_margin(egui::Margin::same(0))
        .show(ui, |ui| {
            palette_input(ui);
            divider(ui);
            palette_result_row(ui, "black fabric folds", "Library");
            palette_result_row(ui, "warm rim light", "This Board");
            palette_footer(ui);
        });
}

fn palette_input(ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_3 as i8,
            right: crate::theme::SPACE_3 as i8,
            top: crate::theme::SPACE_2 as i8,
            bottom: crate::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
                kbd_pill(ui, "⌘K");
                ui.label(
                    egui::RichText::new("Search images")
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                );
                ui.with_layout(crate::ui_layout::row_end_center_y_layout(), |ui| {
                    kbd_pill(ui, "esc");
                });
            });
        });
}

fn palette_result_row(ui: &mut egui::Ui, title: &str, scope: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), egui::Sense::hover());
    let thumb = egui::Rect::from_min_size(
        rect.min + egui::vec2(crate::theme::SPACE_3, 7.0),
        egui::vec2(28.0, 24.0),
    );
    ui.painter().rect_filled(
        thumb,
        egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
        crate::theme::SURFACE_ELEVATED,
    );
    ui.painter().text(
        egui::pos2(thumb.right() + crate::theme::SPACE_2, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::TextStyle::Body.resolve(ui.style()),
        crate::theme::TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.right() - crate::theme::SPACE_3, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        scope,
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT_SUBTLE,
    );
}

fn palette_footer(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(38))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_3 as i8,
            right: crate::theme::SPACE_3 as i8,
            top: crate::theme::SPACE_2 as i8,
            bottom: crate::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
                ui.label(
                    egui::RichText::new("This Board")
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_SUBTLE),
                );
                ui.with_layout(crate::ui_layout::row_end_center_y_layout(), |ui| {
                    kbd_pill(ui, "↵");
                    ui.label(
                        egui::RichText::new("place all")
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE),
                    );
                });
            });
        });
}

fn canvas_toolbar(ui: &mut egui::Ui) {
    section_label(ui, "Canvas toolbar");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_1;
        toolbar_button(ui, "S", true);
        toolbar_button(ui, "M", false);
        toolbar_button(ui, "L", false);
        swatch_button(ui, egui::Color32::from_rgb(0xE8, 0x89, 0x73), true);
        swatch_button(ui, egui::Color32::from_rgb(0x7A, 0xC6, 0xA5), false);
        toolbar_button(ui, "BG", true);
    });
}

fn toolbar_button(ui: &mut egui::Ui, label: &str, active: bool) {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(crate::theme::TEXT),
        )
        .fill(if active {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        })
        .stroke(if active {
            crate::theme::stroke_strong()
        } else {
            crate::theme::stroke_subtle()
        })
        .corner_radius(crate::theme::RADIUS_CONTROL)
        .min_size(egui::vec2(30.0, crate::theme::CONTROL_HEIGHT)),
    );
}

fn swatch_button(ui: &mut egui::Ui, color: egui::Color32, active: bool) {
    ui.add(
        egui::Button::new("")
            .fill(color)
            .stroke(if active {
                crate::theme::stroke_strong()
            } else {
                crate::theme::stroke_subtle()
            })
            .corner_radius(crate::theme::RADIUS_CONTROL)
            .min_size(egui::vec2(28.0, crate::theme::CONTROL_HEIGHT)),
    );
}

fn dialog_buttons(ui: &mut egui::Ui) {
    section_label(ui, "Dialog actions");
    subcard(ui, |ui| {
        ui.label(
            egui::RichText::new("Delete board?")
                .text_style(egui::TextStyle::Heading)
                .color(crate::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(
                "Confirmation actions use destructive color only at the decision point.",
            )
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_MUTED),
        );
        ui.add_space(crate::theme::SPACE_2);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
            dialog_button(ui, "Cancel", DialogButtonKind::Secondary);
            dialog_button(ui, "Delete", DialogButtonKind::Destructive);
            dialog_button(ui, "Install", DialogButtonKind::Primary);
        });
    });
}

enum DialogButtonKind {
    Secondary,
    Destructive,
    Primary,
}

fn dialog_button(ui: &mut egui::Ui, label: &str, kind: DialogButtonKind) {
    let (fill, stroke, text) = match kind {
        DialogButtonKind::Secondary => (
            crate::theme::SURFACE_ELEVATED,
            crate::theme::stroke_subtle(),
            crate::theme::TEXT,
        ),
        DialogButtonKind::Destructive => (
            crate::theme::DESTRUCTIVE_FILL,
            egui::Stroke::new(1.0, crate::theme::DESTRUCTIVE_STROKE),
            crate::theme::TEXT,
        ),
        DialogButtonKind::Primary => (
            crate::theme::ACCENT,
            egui::Stroke::new(1.0, crate::theme::ACCENT),
            crate::theme::BACKGROUND,
        ),
    };
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(text),
        )
        .fill(fill)
        .stroke(stroke)
        .corner_radius(crate::theme::RADIUS_INLINE),
    );
}

fn radius_samples(ui: &mut egui::Ui) {
    section_label(ui, "Radius roles");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
        control_sample(ui, "Compact", crate::theme::RADIUS_COMPACT, true);
        control_sample(ui, "Control", crate::theme::RADIUS_CONTROL, false);
        control_sample(ui, "Panel", crate::theme::RADIUS_PANEL, false);
    });
}

fn subcard(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(crate::theme::QUIET_INSET_FILL)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_3 as i8,
            right: crate::theme::SPACE_3 as i8,
            top: crate::theme::SPACE_2 as i8,
            bottom: crate::theme::SPACE_2 as i8,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            add_contents(ui);
        });
}

fn kbd_pill(ui: &mut egui::Ui, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(30.0, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        crate::theme::RADIUS_INLINE,
        egui::Color32::from_white_alpha(15),
    );
    ui.painter().rect_stroke(
        rect,
        crate::theme::RADIUS_INLINE,
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(15)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(10.0),
        crate::theme::TEXT_MUTED,
    );
}

fn divider(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, crate::theme::RADIUS_NONE, crate::theme::BORDER);
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
    let size = egui::vec2(84.0, 28.0);
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
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT,
    );
}
