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
                        egui::RichText::new("Reference design system")
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

pub(crate) fn show_compact_controls_comparison(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 1500.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
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

    egui::Area::new(egui::Id::new("compact_controls_comparison"))
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
                        egui::RichText::new("Compact controls comparison")
                            .text_style(crate::theme::display_text_style())
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Decision: consolidate small choices, toggles, key hints, and toolbar controls without making the app feel heavier.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                    );

                    comparison_grid(ui);
                    recommendation(ui);
                });
        });
}

pub(crate) fn show_radius_comparison(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 1040.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
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

    egui::Area::new(egui::Id::new("radius_comparison"))
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
                        egui::RichText::new("Radius semantics")
                            .text_style(crate::theme::display_text_style())
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Decision: reduce near-duplicate radii while preserving useful differences between structure, inline controls, explicit controls, and floating panels.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                    );

                    ui.columns(2, |columns| {
                        current_radius_scale(&mut columns[0]);
                        proposed_radius_scale(&mut columns[1]);
                    });
                    radius_recommendation(ui);
                });
        });
}

pub(crate) fn show_destructive_actions_comparison(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 1040.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
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

    egui::Area::new(egui::Id::new("destructive_actions_comparison"))
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
                        egui::RichText::new("Destructive actions")
                            .text_style(crate::theme::display_text_style())
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Decision: destructive actions should stay quiet in repeated rows, become explicit in menus, and become unmistakable at confirmation.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                    );

                    ui.columns(3, |columns| {
                        destructive_variant_current(&mut columns[0]);
                        destructive_variant_semantic(&mut columns[1]);
                        destructive_variant_warning(&mut columns[2]);
                    });
                    destructive_recommendation(ui);
                });
        });
}

pub(crate) fn show_spacing_comparison(ui: &mut egui::Ui, screen_rect: egui::Rect) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 1040.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
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

    egui::Area::new(egui::Id::new("spacing_comparison"))
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
                        egui::RichText::new("Spacing scale audit")
                            .text_style(crate::theme::display_text_style())
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(
                            "Decision: reduce spacing drift without losing the compact, optical adjustments that make the desktop UI feel precise.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::theme::TEXT_MUTED),
                    );

                    ui.columns(3, |columns| {
                        spacing_variant_current(&mut columns[0]);
                        spacing_variant_semantic(&mut columns[1]);
                        spacing_variant_strict(&mut columns[2]);
                    });
                    spacing_recommendation(ui);
                });
        });
}

pub(crate) fn show_board_rows_comparison(
    ui: &mut egui::Ui,
    screen_rect: egui::Rect,
    scroll_offset: f32,
) {
    ui.painter()
        .rect_filled(screen_rect, 0, crate::theme::BACKGROUND);

    let panel_width = 1040.0_f32.min(screen_rect.width() - crate::theme::SPACE_8 * 2.0);
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

    egui::Area::new(egui::Id::new("board_rows_comparison"))
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
                    egui::ScrollArea::vertical()
                        .id_salt("board_rows_comparison_scroll")
                        .vertical_scroll_offset(scroll_offset)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing =
                                egui::vec2(crate::theme::SPACE_4, crate::theme::SPACE_3);
                            ui.label(
                                egui::RichText::new("Sidebar board-row component")
                                    .text_style(crate::theme::display_text_style())
                                    .color(crate::theme::TEXT),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Decision: make board switching, active state, counts, shortcuts, rename, delete, and create affordances easier to scan without making the sidebar feel heavy.",
                                )
                                .text_style(egui::TextStyle::Body)
                                .color(crate::theme::TEXT_MUTED),
                            );

                            board_rows_variant_current(ui);
                            board_rows_variant_recommended(ui);
                            board_rows_variant_spacious(ui);
                            board_rows_recommendation(ui);
                        });
                });
        });
}

fn color_tokens(ui: &mut egui::Ui) {
    section_label(ui, "Color roles");
    let tokens = [
        (
            "Background",
            crate::theme::BACKGROUND,
            "Canvas and app backdrop",
        ),
        (
            "Surface",
            crate::theme::SURFACE,
            "Panels and resting controls",
        ),
        (
            "Elevated",
            crate::theme::SURFACE_ELEVATED,
            "Hover and selected surfaces",
        ),
        (
            "Accent",
            crate::theme::ACCENT,
            "Active focus and selection emphasis",
        ),
        ("Text", crate::theme::TEXT, "Primary readable copy"),
        ("Muted", crate::theme::TEXT_MUTED, "Secondary values"),
        ("Subtle", crate::theme::TEXT_SUBTLE, "Low-emphasis metadata"),
        (
            "Destructive",
            crate::theme::DESTRUCTIVE,
            "Danger labels and confirmations",
        ),
    ];

    egui::Grid::new("design_system_colors")
        .num_columns(3)
        .spacing(egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_1))
        .show(ui, |ui| {
            for (name, color, use_case) in tokens {
                let (swatch_rect, _) =
                    ui.allocate_exact_size(egui::vec2(24.0, 18.0), egui::Sense::hover());
                ui.painter().rect(
                    swatch_rect,
                    egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
                    color,
                    crate::theme::stroke_subtle(),
                    egui::StrokeKind::Inside,
                );
                ui.label(egui::RichText::new(name).color(crate::theme::TEXT));
                ui.label(
                    egui::RichText::new(use_case)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_MUTED),
                );
                ui.end_row();
            }
        });
}

fn board_rows_variant_current(ui: &mut egui::Ui) {
    board_rows_variant_frame(
        ui,
        "A",
        "Current compact row",
        "Keeps the existing 36px list-item model. Highest density, but the active dot, shortcut, delete action, and title compete in the same small band.",
        |ui| {
            board_rows_state_grid(ui, BoardRowVariant::Current);
        },
    );
}

fn board_rows_variant_recommended(ui: &mut egui::Ui) {
    board_rows_variant_frame(
        ui,
        "B",
        "Recommended: navigational row",
        "Treat the board row as navigation first: clearer left active rail, calmer metadata, hidden-until-relevant destructive action, and a create row that belongs to the same list.",
        |ui| {
            board_rows_state_grid(ui, BoardRowVariant::Navigational);
        },
    );
}

fn board_rows_variant_spacious(ui: &mut egui::Ui) {
    board_rows_variant_frame(
        ui,
        "C",
        "Spacious board card",
        "Gives every board more breathing room and moves metadata into chips. Easier to read, but too tall for a dense sidebar with many boards.",
        |ui| {
            board_rows_state_grid(ui, BoardRowVariant::Spacious);
        },
    );
}

fn board_rows_variant_frame(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    rationale: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing =
                egui::vec2(crate::theme::SPACE_3, crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                badge(ui, id, crate::theme::ACCENT, crate::theme::BACKGROUND);
                ui.label(
                    egui::RichText::new(title)
                        .text_style(egui::TextStyle::Heading)
                        .color(crate::theme::TEXT),
                );
            });
            ui.label(
                egui::RichText::new(rationale)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_1);
            add_contents(ui);
        });
}

#[derive(Clone, Copy)]
enum BoardRowVariant {
    Current,
    Navigational,
    Spacious,
}

#[derive(Clone, Copy)]
enum BoardRowState {
    Active,
    Idle,
    Hover,
    Renaming,
}

fn board_rows_state_grid(ui: &mut egui::Ui, variant: BoardRowVariant) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_2 as i8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(520.0);
                board_rows_state_legend(ui);
                ui.add_space(crate::theme::SPACE_1);
                let rows = [
                    ("My Board", "107 images", "Cmd 1", BoardRowState::Active),
                    ("Board 2", "0 images", "Cmd 2", BoardRowState::Idle),
                    ("Client Pulls", "24 images", "Cmd 3", BoardRowState::Hover),
                    (
                        "Mood references",
                        "8 images",
                        "Cmd 4",
                        BoardRowState::Renaming,
                    ),
                ];
                for (index, (name, detail, shortcut, state)) in rows.iter().enumerate() {
                    paint_board_row_variant(ui, variant, name, detail, shortcut, *state);
                    if index < rows.len() - 1 {
                        ui.add_space(crate::theme::SPACE_1);
                    }
                }
                ui.add_space(crate::theme::SPACE_2);
                paint_create_board_variant(ui, variant);
            });
        });
}

fn board_rows_state_legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
        for label in ["active", "idle", "hover/focus", "renaming", "create"] {
            ui.label(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE),
            );
        }
    });
}

fn paint_board_row_variant(
    ui: &mut egui::Ui,
    variant: BoardRowVariant,
    name: &str,
    detail: &str,
    shortcut: &str,
    state: BoardRowState,
) {
    match variant {
        BoardRowVariant::Current => paint_board_row_current(ui, name, detail, shortcut, state),
        BoardRowVariant::Navigational => {
            paint_board_row_navigational(ui, name, detail, shortcut, state);
        }
        BoardRowVariant::Spacious => paint_board_row_spacious(ui, name, detail, shortcut, state),
    }
}

fn paint_board_row_current(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    shortcut: &str,
    state: BoardRowState,
) {
    let active = matches!(state, BoardRowState::Active);
    let hot = active || matches!(state, BoardRowState::Hover | BoardRowState::Renaming);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        if hot {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        },
    );
    let swatch = egui::Rect::from_min_size(
        egui::pos2(rect.left() + crate::theme::SPACE_2, rect.top() + 7.0),
        egui::vec2(22.0, 22.0),
    );
    ui.painter().rect_filled(
        swatch,
        egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
        egui::Color32::from_white_alpha(if active { 34 } else { 18 }),
    );
    let text_x = swatch.right() + crate::theme::SPACE_2;
    paint_board_row_text(
        ui,
        egui::pos2(text_x, rect.top() + 11.0),
        name,
        crate::theme::TEXT,
    );
    paint_board_row_small_left(
        ui,
        egui::pos2(text_x, rect.bottom() - 9.0),
        detail,
        crate::theme::TEXT_SUBTLE,
    );
    let right = rect.right() - crate::theme::SPACE_2;
    paint_board_row_small(
        ui,
        egui::pos2(right - 44.0, rect.center().y),
        shortcut,
        crate::theme::TEXT_SUBTLE,
    );
    if active {
        ui.painter().circle_filled(
            egui::pos2(right - 22.0, rect.center().y),
            2.8,
            crate::theme::ACCENT,
        );
    }
    paint_board_row_small(
        ui,
        egui::pos2(right, rect.center().y),
        "x",
        crate::theme::TEXT_SUBTLE,
    );
}

fn paint_board_row_navigational(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    shortcut: &str,
    state: BoardRowState,
) {
    let active = matches!(state, BoardRowState::Active);
    let hover = matches!(state, BoardRowState::Hover | BoardRowState::Renaming);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        if active || hover {
            crate::theme::SURFACE_ELEVATED
        } else {
            egui::Color32::TRANSPARENT
        },
        if active {
            crate::theme::stroke_subtle()
        } else {
            egui::Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    if active {
        let rail = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(0.0, crate::theme::SPACE_2),
            egui::pos2(rect.left() + 2.0, rect.bottom() - crate::theme::SPACE_2),
        );
        ui.painter()
            .rect_filled(rail, crate::theme::RADIUS_MICRO, crate::theme::ACCENT);
    }
    let swatch = egui::Rect::from_min_size(
        egui::pos2(rect.left() + crate::theme::SPACE_3, rect.top() + 10.0),
        egui::vec2(24.0, 24.0),
    );
    ui.painter().rect_filled(
        swatch,
        egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
        egui::Color32::from_white_alpha(if active { 38 } else { 18 }),
    );
    let text_x = swatch.right() + crate::theme::SPACE_2;
    let title = if matches!(state, BoardRowState::Renaming) {
        "Mood references|"
    } else {
        name
    };
    paint_board_row_text(
        ui,
        egui::pos2(text_x, rect.top() + 13.0),
        title,
        crate::theme::TEXT,
    );
    paint_board_row_small_left(
        ui,
        egui::pos2(text_x, rect.bottom() - 10.0),
        detail,
        crate::theme::TEXT_MUTED,
    );
    let action_color = if hover {
        crate::theme::DESTRUCTIVE
    } else {
        crate::theme::TEXT_SUBTLE
    };
    paint_board_row_small(
        ui,
        egui::pos2(rect.right() - crate::theme::SPACE_3, rect.center().y),
        if hover { "Delete" } else { shortcut },
        action_color,
    );
}

fn paint_board_row_spacious(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    shortcut: &str,
    state: BoardRowState,
) {
    let active = matches!(state, BoardRowState::Active);
    let hover = matches!(state, BoardRowState::Hover | BoardRowState::Renaming);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 58.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_CONTROL),
        if active {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        },
        if active || hover {
            crate::theme::stroke_subtle()
        } else {
            egui::Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    let swatch = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + crate::theme::SPACE_3,
            rect.top() + crate::theme::SPACE_3,
        ),
        egui::vec2(30.0, 30.0),
    );
    ui.painter().rect_filled(
        swatch,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        egui::Color32::from_white_alpha(if active { 42 } else { 20 }),
    );
    let text_x = swatch.right() + crate::theme::SPACE_3;
    paint_board_row_text(
        ui,
        egui::pos2(text_x, rect.top() + 18.0),
        name,
        crate::theme::TEXT,
    );
    board_meta_chip(
        ui,
        egui::pos2(text_x, rect.bottom() - 15.0),
        detail,
        egui::Color32::from_white_alpha(10),
    );
    board_meta_chip(
        ui,
        egui::pos2(rect.right() - 58.0, rect.bottom() - 15.0),
        shortcut,
        egui::Color32::TRANSPARENT,
    );
    paint_board_row_small(
        ui,
        egui::pos2(rect.right() - crate::theme::SPACE_3, rect.top() + 18.0),
        if hover { "..." } else { "" },
        crate::theme::TEXT_SUBTLE,
    );
}

fn paint_create_board_variant(ui: &mut egui::Ui, variant: BoardRowVariant) {
    let height = match variant {
        BoardRowVariant::Current => 30.0,
        BoardRowVariant::Navigational => 36.0,
        BoardRowVariant::Spacious => 42.0,
    };
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(match variant {
            BoardRowVariant::Spacious => crate::theme::RADIUS_CONTROL,
            _ => crate::theme::RADIUS_COMPACT,
        }),
        match variant {
            BoardRowVariant::Navigational => egui::Color32::from_white_alpha(4),
            _ => egui::Color32::TRANSPARENT,
        },
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    let label = match variant {
        BoardRowVariant::Current => "+  Create new board",
        BoardRowVariant::Navigational => "+  New board",
        BoardRowVariant::Spacious => "Create board",
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::TextStyle::Body.resolve(ui.style()),
        crate::theme::TEXT_SUBTLE,
    );
}

fn paint_board_row_text(ui: &mut egui::Ui, pos: egui::Pos2, text: &str, color: egui::Color32) {
    ui.painter().text(
        pos,
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Body.resolve(ui.style()),
        color,
    );
}

fn paint_board_row_small(ui: &mut egui::Ui, pos: egui::Pos2, text: &str, color: egui::Color32) {
    ui.painter().text(
        pos,
        egui::Align2::RIGHT_CENTER,
        text,
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
}

fn paint_board_row_small_left(
    ui: &mut egui::Ui,
    pos: egui::Pos2,
    text: &str,
    color: egui::Color32,
) {
    ui.painter().text(
        pos,
        egui::Align2::LEFT_CENTER,
        text,
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
}

fn board_meta_chip(ui: &mut egui::Ui, center: egui::Pos2, text: &str, fill: egui::Color32) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT_SUBTLE,
    );
    let rect = egui::Rect::from_center_size(
        center + egui::vec2(galley.size().x * 0.5, 0.0),
        galley.size() + egui::vec2(10.0, 4.0),
    );
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
        fill,
    );
    ui.painter().galley(
        crate::ui_layout::centered_pos(rect, galley.size()),
        galley,
        crate::theme::TEXT_SUBTLE,
    );
}

fn board_rows_recommendation(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Recommendation: B Navigational row")
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Use the board row as a navigation component, not a tiny card. Keep the row compact, add a clear active rail, reserve filled treatment for active/hover, show shortcuts as metadata, and reveal destructive language only when the row is hovered or focused.",
                )
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn type_scale(ui: &mut egui::Ui) {
    section_label(ui, "Type scale");
    let samples = [
        (
            "Display",
            crate::theme::display_text_style(),
            "Section headers and compact panel titles",
        ),
        (
            "Heading",
            egui::TextStyle::Heading,
            "Dialog titles and rare hierarchy breaks",
        ),
        (
            "Body",
            egui::TextStyle::Body,
            "Default labels and readable panel copy",
        ),
        ("Button", egui::TextStyle::Button, "Compact action text"),
        (
            "Small",
            egui::TextStyle::Small,
            "Hints, shortcut labels, metadata",
        ),
    ];

    egui::Grid::new("design_system_type")
        .num_columns(2)
        .spacing(egui::vec2(crate::theme::SPACE_3, crate::theme::SPACE_2))
        .show(ui, |ui| {
            for (name, style, use_case) in samples {
                ui.label(
                    egui::RichText::new(name)
                        .text_style(style)
                        .color(crate::theme::TEXT),
                );
                ui.label(
                    egui::RichText::new(use_case)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_SUBTLE),
                );
                ui.end_row();
            }
        });
}

fn controls_and_surfaces(ui: &mut egui::Ui) {
    section_label(ui, "Controls and surfaces");
    crate::ui_layout::row_center_y(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(crate::theme::SPACE_3, crate::theme::SPACE_2);
        let _ = ui.add_sized(
            [116.0, crate::theme::CONTROL_HEIGHT],
            egui::Button::new("Primary action"),
        );
        let _ = ui.add_sized(
            [112.0, crate::theme::CONTROL_HEIGHT],
            egui::Button::new("Quiet action").frame(false),
        );
        quiet_text_control(ui, "On", true);
        quiet_text_control(ui, "Off", false);
    });

    crate::ui_layout::row_center_y(ui, |ui| {
        surface_sample(
            ui,
            "Sidebar card",
            crate::theme::SURFACE,
            "Grouped settings and board rows",
        );
        surface_sample(
            ui,
            "Selected tile",
            crate::theme::SURFACE_ELEVATED,
            "Active board, hover, search lead",
        );
        surface_sample(
            ui,
            "Palette",
            crate::theme::PALETTE_SURFACE,
            "Search overlay only",
        );
    });

    alignment_samples(ui);
}

fn alignment_samples(ui: &mut egui::Ui) {
    section_label(ui, "Alignment helpers");
    ui.columns(3, |columns| {
        alignment_sample_card(&mut columns[0], "row_center_y", "Inline controls", |ui| {
            crate::ui_layout::row_center_y(ui, |ui| {
                ui.label(egui::RichText::new("Label").color(crate::theme::TEXT_MUTED));
                quiet_text_control(ui, "On", true);
                let _ = ui.add_sized(
                    [76.0, crate::theme::CONTROL_HEIGHT],
                    egui::Button::new("Apply"),
                );
            });
        });
        alignment_sample_card(
            &mut columns[1],
            "row_end_center_y",
            "Trailing actions",
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), crate::theme::CONTROL_HEIGHT),
                    crate::ui_layout::row_end_center_y_layout(),
                    |ui| {
                        let _ = ui.add_sized(
                            [76.0, crate::theme::CONTROL_HEIGHT],
                            egui::Button::new("Delete"),
                        );
                        let _ = ui.add_sized(
                            [76.0, crate::theme::CONTROL_HEIGHT],
                            egui::Button::new("Cancel").frame(false),
                        );
                    },
                );
            },
        );
        alignment_sample_card(
            &mut columns[2],
            "column_center_x",
            "Brief status only",
            |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 52.0),
                    crate::ui_layout::column_center_x(),
                    |ui| {
                        ui.label(egui::RichText::new("Ready").color(crate::theme::TEXT));
                        ui.label(
                            egui::RichText::new("Centered by exception")
                                .text_style(egui::TextStyle::Small)
                                .color(crate::theme::TEXT_SUBTLE),
                        );
                    },
                );
            },
        );
    });
}

fn alignment_sample_card(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(title)
                        .text_style(egui::TextStyle::Small)
                        .strong()
                        .color(crate::theme::TEXT),
                );
                ui.label(
                    egui::RichText::new(detail)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT_SUBTLE),
                );
                ui.add_space(crate::theme::SPACE_2);
                add_contents(ui);
            });
        });
}

fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Heading)
            .color(crate::theme::TEXT),
    );
}

fn surface_sample(ui: &mut egui::Ui, title: &str, fill: egui::Color32, detail: &str) {
    egui::Frame::new()
        .fill(fill)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.set_width(190.0);
            ui.label(egui::RichText::new(title).color(crate::theme::TEXT));
            ui.label(
                egui::RichText::new(detail)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn current_radius_scale(ui: &mut egui::Ui) {
    radius_panel(
        ui,
        "Previous spread",
        "Eight visible radius values were in active use. Some were distinct; 5/6/7 were mostly local optical choices.",
        |ui| {
            radius_specimen(
                ui,
                "0 Structural",
                0,
                "Canvas fills, dividers, tab underline",
            );
            radius_specimen(ui, "3 Micro", 3, "Tiny swatches, active dot, image thumb");
            radius_specimen(ui, "4 Inline", 4, "Quiet setting choices, key hints");
            radius_specimen(ui, "5 Row", 5, "Board rows and create-board row");
            radius_specimen(ui, "6 Chip", 6, "Palette scope/pivot pills, sidebar hint");
            radius_specimen(ui, "7 Group", 7, "Sidebar subcard, inset samples");
            radius_specimen(ui, "8 Control", 8, "Buttons, canvas toolbar targets");
            radius_specimen(ui, "12 Panel", 12, "Sidebar outer edge, palette, preview");
        },
    );
}

fn proposed_radius_scale(ui: &mut egui::Ui) {
    radius_panel(
        ui,
        "Selected scale",
        "Keep five semantic levels plus square. Previous 5/6/7 values now use one compact-container radius.",
        |ui| {
            radius_context_card(
                ui,
                "R0 Structural",
                0,
                "Use for full-bleed fills, rules, underlines, grid/canvas geometry.",
            );
            radius_context_card(
                ui,
                "R3 Micro",
                3,
                "Use for tiny marks, dots, thumbnails, and swatches under 24px.",
            );
            radius_context_card(
                ui,
                "R4 Inline",
                4,
                "Use for quiet inline setting choices where text stays primary.",
            );
            radius_context_card(
                ui,
                "R6 Compact",
                6,
                "Merge rows, chips, hints, and inset groups here by default.",
            );
            radius_context_card(
                ui,
                "R8 Control",
                8,
                "Use for explicit buttons and direct manipulation toolbar targets.",
            );
            radius_context_card(
                ui,
                "R12 Panel",
                12,
                "Use for floating overlays, palette, sidebar shell, major previews.",
            );
        },
    );
}

fn radius_panel(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing =
                egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2);
            ui.label(
                egui::RichText::new(title)
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(detail)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_1);
            add_contents(ui);
        });
}

fn radius_specimen(ui: &mut egui::Ui, label: &str, radius: u8, detail: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(74.0, 34.0), egui::Sense::hover());
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(radius),
            if radius == 0 {
                crate::theme::BACKGROUND
            } else {
                crate::theme::SURFACE_ELEVATED
            },
            crate::theme::stroke_subtle(),
            egui::StrokeKind::Inside,
        );
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Button)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(detail)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE),
            );
        });
    });
}

fn radius_context_card(ui: &mut egui::Ui, label: &str, radius: u8, detail: &str) {
    egui::Frame::new()
        .fill(if radius == 0 {
            crate::theme::BACKGROUND
        } else {
            crate::theme::SURFACE_ELEVATED
        })
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(radius))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_2 as i8))
        .show(ui, |ui| {
            ui.set_min_width(440.0);
            ui.horizontal(|ui| {
                let (sample_rect, _) =
                    ui.allocate_exact_size(egui::vec2(58.0, 28.0), egui::Sense::hover());
                ui.painter().rect(
                    sample_rect,
                    egui::CornerRadius::same(radius),
                    crate::theme::SURFACE,
                    crate::theme::stroke_subtle(),
                    egui::StrokeKind::Inside,
                );
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .text_style(egui::TextStyle::Button)
                            .color(crate::theme::TEXT),
                    );
                    ui.label(
                        egui::RichText::new(detail)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_MUTED),
                    );
                });
            });
        });
}

fn radius_recommendation(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Applied: merge 5, 6, and 7 into R6 Compact")
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "R5 row, R6 chip, and R7 group are too close to carry separate meaning. Keep R4 for quiet inline controls, R8 for explicit toolbar/buttons, and R12 for floating panels.",
                )
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn spacing_variant_current(ui: &mut egui::Ui) {
    spacing_variant_frame(
        ui,
        "A",
        "Current broad scale",
        "Tokens follow 4px steps, but the app still uses local 1/2/3/5/6/9/10/14/18/22px values for optical tuning.",
        |ui| {
            spacing_scale_bars(ui, &[4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0]);
            spacing_sample_card(ui, 16.0, 12.0, 8.0, "Panel");
            spacing_sample_card(ui, 12.0, 10.0, 4.0, "Sidebar");
            spacing_sample_card(ui, 9.0, 6.0, 3.0, "Palette chip");
            affected(
                ui,
                "Good coverage; weak semantic boundary around 20px and many raw near-token values.",
            );
        },
    );
}

fn spacing_variant_semantic(ui: &mut egui::Ui) {
    spacing_variant_frame(
        ui,
        "B",
        "Semantic merge",
        "Keep the useful 4/8/12/16/24/32 roles. Remove 20px as a token; map near-token values back unless they are true optical nudges.",
        |ui| {
            spacing_scale_bars(ui, &[4.0, 8.0, 12.0, 16.0, 24.0, 32.0]);
            spacing_sample_card(ui, 16.0, 12.0, 8.0, "Panel");
            spacing_sample_card(ui, 12.0, 8.0, 4.0, "Sidebar");
            spacing_sample_card(ui, 8.0, 4.0, 2.0, "Palette chip");
            affected(
                ui,
                "Recommended: dense, predictable, and still allows documented optical 1-3px corrections.",
            );
        },
    );
}

fn spacing_variant_strict(ui: &mut egui::Ui) {
    spacing_variant_frame(
        ui,
        "C",
        "Strict 8px grid",
        "Use only 8/16/24/32 for visible spacing and push all smaller values into component geometry.",
        |ui| {
            spacing_scale_bars(ui, &[8.0, 16.0, 24.0, 32.0]);
            spacing_sample_card(ui, 16.0, 16.0, 8.0, "Panel");
            spacing_sample_card(ui, 16.0, 8.0, 8.0, "Sidebar");
            spacing_sample_card(ui, 8.0, 8.0, 8.0, "Palette chip");
            affected(
                ui,
                "Too coarse for this app; it makes dense controls feel padded and less deliberate.",
            );
        },
    );
}

fn spacing_variant_frame(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    rationale: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing =
                egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                badge(ui, id, crate::theme::ACCENT, crate::theme::BACKGROUND);
                ui.label(
                    egui::RichText::new(title)
                        .text_style(egui::TextStyle::Heading)
                        .color(crate::theme::TEXT),
                );
            });
            ui.label(
                egui::RichText::new(rationale)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_2);
            add_contents(ui);
        });
}

fn spacing_scale_bars(ui: &mut egui::Ui, values: &[f32]) {
    row_label(ui, "Scale");
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_1);
        for value in values {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(*value * 3.0, 8.0), egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
                crate::theme::ACCENT,
            );
            ui.label(
                egui::RichText::new(format!("{value:.0}px"))
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
        }
    });
}

fn spacing_sample_card(
    ui: &mut egui::Ui,
    outer_pad: f32,
    row_gap: f32,
    inline_gap: f32,
    label: &str,
) {
    row_label(ui, label);
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(outer_pad as i8))
        .show(ui, |ui| {
            ui.set_width(250.0 - outer_pad * 2.0);
            ui.spacing_mut().item_spacing = egui::vec2(inline_gap, row_gap);
            ui.label(
                egui::RichText::new("Moodboard")
                    .text_style(egui::TextStyle::Button)
                    .color(crate::theme::TEXT),
            );
            ui.horizontal(|ui| {
                spacing_pill(ui, "Original");
                spacing_pill(ui, "Grid");
                spacing_pill(ui, "24px");
            });
            ui.label(
                egui::RichText::new(format!(
                    "pad {outer_pad:.0} / rows {row_gap:.0} / inline {inline_gap:.0}"
                ))
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_SUBTLE),
            );
        });
}

fn spacing_pill(ui: &mut egui::Ui, label: &str) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_INLINE))
        .inner_margin(egui::Margin {
            left: crate::theme::SPACE_2 as i8,
            right: crate::theme::SPACE_2 as i8,
            top: crate::theme::SPACE_1 as i8,
            bottom: crate::theme::SPACE_1 as i8,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn spacing_recommendation(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Selected: B Semantic merge")
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Keep 4, 8, 12, 16, 24, and 32. Retire 20 as a named token. Treat 1-3px values as documented optical corrections, and map 5/6/9/10/14/18/22 to adjacent tokens unless the local component proves it needs optical tuning.",
                )
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn destructive_variant_current(ui: &mut egui::Ui) {
    destructive_variant_frame(
        ui,
        "A",
        "Current quiet",
        "Deletion is visually hidden until a generic confirmation appears.",
        |ui| {
            board_row_sample(
                ui,
                "Moodboard",
                "12 images",
                "x",
                crate::theme::TEXT_SUBTLE,
                false,
            );
            context_menu_sample(ui, "Remove from Library", crate::theme::TEXT, false);
            confirmation_sample(
                ui,
                "Delete board?",
                "This board and its placements will be removed.",
                "Delete",
                crate::theme::TEXT,
                false,
            );
            affected(
                ui,
                "Low visual noise, but destructive affordance is easy to miss.",
            );
        },
    );
}

fn destructive_variant_semantic(ui: &mut egui::Ui) {
    destructive_variant_frame(
        ui,
        "B",
        "Semantic destructive",
        "Rows remain quiet; menus and confirmations name the destructive action clearly.",
        |ui| {
            board_row_sample(
                ui,
                "Moodboard",
                "12 images",
                "Delete",
                destructive_color(),
                false,
            );
            context_menu_sample(ui, "Remove from Library", destructive_color(), false);
            confirmation_sample(
                ui,
                "Delete board?",
                "This cannot be undone. Images remain in the Library.",
                "Delete board",
                destructive_color(),
                true,
            );
            affected(
                ui,
                "Recommended: explicit language without loud row-level warning.",
            );
        },
    );
}

fn destructive_variant_warning(ui: &mut egui::Ui) {
    destructive_variant_frame(
        ui,
        "C",
        "High-warning",
        "Every destructive target gets strong warning treatment before confirmation.",
        |ui| {
            board_row_sample(
                ui,
                "Moodboard",
                "12 images",
                "Delete",
                destructive_color(),
                true,
            );
            context_menu_sample(ui, "Remove from Library", destructive_color(), true);
            confirmation_sample(
                ui,
                "Delete board?",
                "This cannot be undone. Review before continuing.",
                "Delete board",
                destructive_color(),
                true,
            );
            affected(
                ui,
                "Best for irreversible loss; too loud for repeated board rows.",
            );
        },
    );
}

fn destructive_variant_frame(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    rationale: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing =
                egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                badge(ui, id, crate::theme::ACCENT, crate::theme::BACKGROUND);
                ui.label(
                    egui::RichText::new(title)
                        .text_style(egui::TextStyle::Heading)
                        .color(crate::theme::TEXT),
                );
            });
            ui.label(
                egui::RichText::new(rationale)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_2);
            add_contents(ui);
        });
}

fn board_row_sample(
    ui: &mut egui::Ui,
    name: &str,
    detail: &str,
    action: &str,
    action_color: egui::Color32,
    filled: bool,
) {
    row_label(ui, "Board row");
    let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 42.0), egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        crate::theme::SURFACE_ELEVATED,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + crate::theme::SPACE_3, rect.top() + 12.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::TextStyle::Body.resolve(ui.style()),
        crate::theme::TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left() + crate::theme::SPACE_3, rect.bottom() - 11.0),
        egui::Align2::LEFT_CENTER,
        detail,
        egui::TextStyle::Small.resolve(ui.style()),
        crate::theme::TEXT_SUBTLE,
    );
    let action_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 42.0, rect.center().y),
        egui::vec2(66.0, 24.0),
    );
    if filled {
        ui.painter().rect(
            action_rect,
            egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
            destructive_fill(),
            egui::Stroke::new(1.0, destructive_stroke()),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        action_rect.center(),
        egui::Align2::CENTER_CENTER,
        action,
        egui::TextStyle::Small.resolve(ui.style()),
        if filled {
            crate::theme::TEXT
        } else {
            action_color
        },
    );
}

fn context_menu_sample(
    ui: &mut egui::Ui,
    destructive_label: &str,
    color: egui::Color32,
    filled: bool,
) {
    row_label(ui, "Context menu");
    egui::Frame::new()
        .fill(crate::theme::SURFACE)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_2 as i8))
        .show(ui, |ui| {
            ui.set_width(280.0);
            menu_item(ui, "Copy", crate::theme::TEXT, false);
            menu_item(ui, "Duplicate", crate::theme::TEXT, false);
            menu_item(ui, destructive_label, color, filled);
        });
}

fn menu_item(ui: &mut egui::Ui, label: &str, color: egui::Color32, filled: bool) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    if filled {
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
            destructive_fill(),
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        egui::pos2(rect.left() + crate::theme::SPACE_2, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::TextStyle::Small.resolve(ui.style()),
        color,
    );
}

fn confirmation_sample(
    ui: &mut egui::Ui,
    title: &str,
    body: &str,
    action: &str,
    color: egui::Color32,
    emphasized: bool,
) {
    row_label(ui, "Confirmation");
    egui::Frame::new()
        .fill(crate::theme::SURFACE)
        .stroke(if emphasized {
            egui::Stroke::new(1.0, destructive_stroke())
        } else {
            crate::theme::stroke_subtle()
        })
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_PANEL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.set_width(280.0);
            ui.label(
                egui::RichText::new(title)
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(body)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                quiet_text_control(ui, "Cancel", false);
                destructive_button(ui, action, color, emphasized);
            });
        });
}

fn destructive_button(ui: &mut egui::Ui, label: &str, color: egui::Color32, filled: bool) {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(if filled { crate::theme::TEXT } else { color }),
        )
        .fill(if filled {
            destructive_fill()
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if filled {
            egui::Stroke::new(1.0, destructive_stroke())
        } else {
            egui::Stroke::NONE
        })
        .corner_radius(crate::theme::RADIUS_INLINE)
        .min_size(egui::vec2(88.0, 26.0)),
    );
}

fn destructive_recommendation(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Recommendation: B Semantic destructive")
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Use explicit destructive language and a reserved destructive color in menus and confirmations. Keep repeated row affordances quiet unless hovered or focused.",
                )
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
            );
        });
}

fn destructive_color() -> egui::Color32 {
    crate::theme::DESTRUCTIVE
}

fn destructive_stroke() -> egui::Color32 {
    crate::theme::DESTRUCTIVE_STROKE
}

fn destructive_fill() -> egui::Color32 {
    crate::theme::DESTRUCTIVE_FILL
}

fn comparison_grid(ui: &mut egui::Ui) {
    ui.columns(3, |columns| {
        variant_current(&mut columns[0]);
        variant_unified(&mut columns[1]);
        variant_quiet(&mut columns[2]);
    });
}

fn variant_current(ui: &mut egui::Ui) {
    variant_frame(
        ui,
        "A",
        "Current mixed controls",
        "Shows today’s drift: segmented picker, radio choice, switch, key hint, and toolbar buttons each have separate geometry.",
        |ui| {
            current_segmented(ui);
            current_radio_choice(ui);
            current_switch(ui, true);
            current_key_hint(ui, "esc");
            current_toolbar(ui);
            affected(ui, "Existing settings, palette, shortcuts, canvas toolbar");
        },
    );
}

fn variant_unified(ui: &mut egui::Ui) {
    variant_frame(
        ui,
        "B",
        "Unified compact controls",
        "One compact grammar: shared 28px control height, 6px radius, tokenized fills, and explicit selected states.",
        |ui| {
            unified_segmented(ui);
            unified_radio_choice(ui);
            unified_switch(ui, true);
            unified_key_hint(ui, "esc");
            unified_toolbar(ui);
            affected(
                ui,
                "Recommended base for settings, palette, shortcut hints, toolbars",
            );
        },
    );
}

fn variant_quiet(ui: &mut egui::Ui) {
    variant_frame(
        ui,
        "C",
        "Quiet inline controls",
        "Selected direction: text-led controls with only active state framed. Best fit for read-heavy settings and low-chrome sidebar work.",
        |ui| {
            quiet_segmented(ui);
            quiet_radio_choice(ui);
            quiet_switch(ui, true);
            quiet_key_hint(ui, "esc");
            quiet_toolbar(ui);
            affected(
                ui,
                "Selected for sidebar settings first; palette and toolbar follow after focused checks",
            );
        },
    );
}

fn variant_frame(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    rationale: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from_white_alpha(5))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.set_min_width(0.0);
            ui.spacing_mut().item_spacing =
                egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_2);
            ui.horizontal(|ui| {
                badge(ui, id, crate::theme::ACCENT, crate::theme::BACKGROUND);
                ui.label(
                    egui::RichText::new(title)
                        .text_style(egui::TextStyle::Heading)
                        .color(crate::theme::TEXT),
                );
            });
            ui.label(
                egui::RichText::new(rationale)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_MUTED),
            );
            ui.add_space(crate::theme::SPACE_2);
            add_contents(ui);
        });
}

fn current_segmented(ui: &mut egui::Ui) {
    row_label(ui, "Segmented choice");
    let outer = egui::Frame::new()
        .fill(egui::Color32::from_black_alpha(64))
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (label, selected) in [("None", false), ("Grid", true), ("Dots", false)] {
                    let fill = if selected {
                        egui::Color32::from_white_alpha(20)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    ui.add(
                        egui::Button::new(
                            egui::RichText::new(label)
                                .text_style(egui::TextStyle::Small)
                                .color(if selected {
                                    crate::theme::TEXT
                                } else {
                                    crate::theme::TEXT_MUTED
                                }),
                        )
                        .fill(fill)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(crate::theme::RADIUS_MICRO)
                        .min_size(egui::vec2(62.0, 24.0)),
                    );
                }
            });
        });
    let _ = outer.response;
}

fn current_radio_choice(ui: &mut egui::Ui) {
    row_label(ui, "Radio choice");
    ui.horizontal(|ui| {
        radio_dot(ui, true);
        ui.label(egui::RichText::new("Original").color(crate::theme::TEXT));
        radio_dot(ui, false);
        ui.label(egui::RichText::new("Downscale").color(crate::theme::TEXT));
    });
}

fn current_switch(ui: &mut egui::Ui, on: bool) {
    row_label(ui, "Switch");
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 24.0), egui::Sense::hover());
    let track_rect = rect.shrink2(egui::vec2(1.0, 3.0));
    ui.painter().rect(
        track_rect,
        track_rect.height() * 0.5,
        if on {
            crate::theme::ACCENT
        } else {
            crate::theme::SURFACE_ELEVATED
        },
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Middle,
    );
    let knob_x = if on {
        rect.right() - 12.0
    } else {
        rect.left() + 12.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 9.0, crate::theme::TEXT);
}

fn current_key_hint(ui: &mut egui::Ui, label: &str) {
    row_label(ui, "Key hint");
    key_box(
        ui,
        label,
        egui::vec2(28.0, 28.0),
        4,
        crate::theme::TEXT_MUTED,
    );
}

fn current_toolbar(ui: &mut egui::Ui) {
    row_label(ui, "Toolbar");
    ui.horizontal(|ui| {
        toolbar_button(ui, "S", false, 34.0, crate::theme::RADIUS_CONTROL);
        toolbar_button(ui, "M", true, 34.0, crate::theme::RADIUS_CONTROL);
        color_swatch(ui, egui::Color32::from_rgb(106, 184, 255), true);
        toolbar_button(ui, "BG", false, 42.0, crate::theme::RADIUS_CONTROL);
    });
}

fn unified_segmented(ui: &mut egui::Ui) {
    row_label(ui, "Segmented choice");
    egui::Frame::new()
        .fill(crate::theme::BACKGROUND)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (label, selected) in [("None", false), ("Grid", true), ("Dots", false)] {
                    compact_button(ui, label, selected, 62.0);
                }
            });
        });
}

fn unified_radio_choice(ui: &mut egui::Ui) {
    row_label(ui, "Radio choice");
    ui.horizontal(|ui| {
        unified_choice(ui, "Original", true);
        unified_choice(ui, "Downscale", false);
    });
}

fn unified_switch(ui: &mut egui::Ui, on: bool) {
    row_label(ui, "Switch");
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 24.0), egui::Sense::hover());
    let track_rect = rect.shrink2(egui::vec2(0.0, 2.0));
    ui.painter().rect(
        track_rect,
        6,
        if on {
            crate::theme::ACCENT
        } else {
            crate::theme::SURFACE_ELEVATED
        },
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    let knob_x = if on {
        rect.right() - 12.0
    } else {
        rect.left() + 12.0
    };
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), 8.0, crate::theme::TEXT);
}

fn unified_key_hint(ui: &mut egui::Ui, label: &str) {
    row_label(ui, "Key hint");
    key_box(
        ui,
        label,
        egui::vec2(30.0, 24.0),
        6,
        crate::theme::TEXT_MUTED,
    );
}

fn unified_toolbar(ui: &mut egui::Ui) {
    row_label(ui, "Toolbar");
    ui.horizontal(|ui| {
        compact_button(ui, "S", false, 34.0);
        compact_button(ui, "M", true, 34.0);
        color_swatch(ui, egui::Color32::from_rgb(106, 184, 255), true);
        compact_button(ui, "BG", false, 42.0);
    });
}

fn quiet_segmented(ui: &mut egui::Ui) {
    row_label(ui, "Segmented choice");
    ui.horizontal(|ui| {
        for (label, selected) in [("None", false), ("Grid", true), ("Dots", false)] {
            quiet_text_control(ui, label, selected);
        }
    });
}

fn quiet_radio_choice(ui: &mut egui::Ui) {
    row_label(ui, "Radio choice");
    ui.horizontal(|ui| {
        quiet_text_control(ui, "Original", true);
        quiet_text_control(ui, "Downscale", false);
    });
}

fn quiet_switch(ui: &mut egui::Ui, on: bool) {
    row_label(ui, "Switch");
    ui.horizontal(|ui| {
        quiet_text_control(ui, if on { "On" } else { "Off" }, on);
        ui.label(
            egui::RichText::new("Semantic search")
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
        );
    });
}

fn quiet_key_hint(ui: &mut egui::Ui, label: &str) {
    row_label(ui, "Key hint");
    key_box(
        ui,
        label,
        egui::vec2(28.0, 22.0),
        3,
        crate::theme::TEXT_SUBTLE,
    );
}

fn quiet_toolbar(ui: &mut egui::Ui) {
    row_label(ui, "Toolbar");
    ui.horizontal(|ui| {
        quiet_text_control(ui, "S", false);
        quiet_text_control(ui, "M", true);
        color_swatch(ui, egui::Color32::from_rgb(106, 184, 255), true);
        quiet_text_control(ui, "BG", false);
    });
}

fn row_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_SUBTLE),
    );
}

fn compact_button(ui: &mut egui::Ui, label: &str, selected: bool, width: f32) {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(if selected {
                    crate::theme::TEXT
                } else {
                    crate::theme::TEXT_MUTED
                }),
        )
        .fill(if selected {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        })
        .stroke(if selected {
            crate::theme::stroke_strong()
        } else {
            crate::theme::stroke_subtle()
        })
        .corner_radius(crate::theme::RADIUS_COMPACT)
        .min_size(egui::vec2(width, crate::theme::CONTROL_HEIGHT)),
    );
}

fn toolbar_button(ui: &mut egui::Ui, label: &str, selected: bool, width: f32, radius: u8) {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(crate::theme::TEXT),
        )
        .fill(if selected {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        })
        .stroke(if selected {
            crate::theme::stroke_strong()
        } else {
            crate::theme::stroke_subtle()
        })
        .corner_radius(radius)
        .min_size(egui::vec2(width, crate::theme::CONTROL_HEIGHT)),
    );
}

fn quiet_text_control(ui: &mut egui::Ui, label: &str, selected: bool) {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .text_style(egui::TextStyle::Button)
                .color(if selected {
                    crate::theme::TEXT
                } else {
                    crate::theme::TEXT_MUTED
                }),
        )
        .fill(if selected {
            egui::Color32::from_white_alpha(12)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if selected {
            crate::theme::stroke_subtle()
        } else {
            egui::Stroke::NONE
        })
        .corner_radius(crate::theme::RADIUS_INLINE)
        .min_size(egui::vec2(34.0, 24.0)),
    );
}

fn unified_choice(ui: &mut egui::Ui, label: &str, selected: bool) {
    egui::Frame::new()
        .fill(if selected {
            crate::theme::SURFACE_ELEVATED
        } else {
            crate::theme::SURFACE
        })
        .stroke(if selected {
            crate::theme::stroke_strong()
        } else {
            crate::theme::stroke_subtle()
        })
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
        .inner_margin(egui::Margin {
            left: 8,
            right: 10,
            top: 4,
            bottom: 4,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                radio_dot(ui, selected);
                ui.label(
                    egui::RichText::new(label)
                        .text_style(egui::TextStyle::Small)
                        .color(if selected {
                            crate::theme::TEXT
                        } else {
                            crate::theme::TEXT_MUTED
                        }),
                );
            });
        });
}

fn radio_dot(ui: &mut egui::Ui, selected: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_stroke(
        rect.center(),
        5.0,
        egui::Stroke::new(1.4, egui::Color32::from_white_alpha(90)),
    );
    if selected {
        ui.painter()
            .circle_filled(rect.center(), 2.8, crate::theme::ACCENT);
    }
}

fn color_swatch(ui: &mut egui::Ui, color: egui::Color32, selected: bool) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(34.0, crate::theme::CONTROL_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect(
        rect,
        6,
        color,
        if selected {
            crate::theme::stroke_strong()
        } else {
            crate::theme::stroke_subtle()
        },
        egui::StrokeKind::Inside,
    );
}

fn key_box(ui: &mut egui::Ui, label: &str, size: egui::Vec2, radius: u8, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        radius,
        egui::Color32::from_white_alpha(15),
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::monospace(10.0),
        color,
    );
}

fn badge(ui: &mut egui::Ui, label: &str, fill: egui::Color32, text: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        fill,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::TextStyle::Small.resolve(ui.style()),
        text,
    );
}

fn affected(ui: &mut egui::Ui, text: &str) {
    ui.add_space(crate::theme::SPACE_1);
    ui.label(
        egui::RichText::new(text)
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_SUBTLE),
    );
}

fn recommendation(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
        .inner_margin(egui::Margin::same(crate::theme::SPACE_3 as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Selected: C Quiet inline controls")
                    .text_style(egui::TextStyle::Heading)
                    .color(crate::theme::TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "This keeps settings dense and low-chrome. Apply it first to sidebar setting controls; keep stronger framed toolbar controls where direct canvas manipulation needs a clearer target.",
                )
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
            );
        });
}
