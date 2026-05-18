use eframe::egui;

use super::{
    SemanticSearchSettingsAction, SemanticSearchSettingsStatus, SemanticSearchSettingsView,
};

pub(super) struct SettingsSectionOutcome {
    pub(super) changed: bool,
    pub(super) semantic_search_action: Option<SemanticSearchSettingsAction>,
}

pub(super) fn paint_settings_section(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    semantic_search: &SemanticSearchSettingsView,
) -> SettingsSectionOutcome {
    let mut changed = false;

    changed |= paint_setting_row(
        ui,
        "Pointer mode",
        pointer_mode_detail(settings.pointer_mode),
        |ui| paint_pointer_mode_control(ui, &mut settings.pointer_mode),
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Always on top",
        "Keep SnappyRef above normal windows.",
        |ui| paint_switch(ui, &mut settings.always_on_top).changed(),
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(ui, "Canvas background", "Quiet board guide.", |ui| {
        paint_background_mode_control(ui, canvas)
    });
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Rotation snap",
        "Default snap angle for image rotation.",
        |ui| {
            paint_number_text_field(
                ui,
                "rotation-snap-degrees",
                &mut settings.rotation_snap_degrees,
                0.0..=180.0,
                "deg",
            )
        },
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Textbox background",
        "Default background for new textboxes.",
        |ui| paint_switch(ui, &mut settings.default_textbox_background).changed(),
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Auto-scale new images",
        "Caps new imports to a default area.",
        |ui| {
            #[cfg(feature = "semantic-search")]
            {
                paint_switch(ui, &mut settings.auto_image_scaling).changed()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                ui.add_enabled(
                    false,
                    egui::widgets::Checkbox::without_text(&mut settings.auto_image_scaling),
                )
                .changed()
            }
        },
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(ui, "Image gap", "Gap used when placing new images.", |ui| {
        paint_number_text_field(
            ui,
            "image-gap",
            &mut settings.import_gap,
            0.0..=128.0,
            "units",
        )
    });

    paint_setting_divider(ui);
    let semantic_search_action = paint_semantic_search_settings_section(ui, semantic_search);

    SettingsSectionOutcome {
        changed,
        semantic_search_action,
    }
}

fn paint_semantic_search_settings_section(
    ui: &mut egui::Ui,
    semantic_search: &SemanticSearchSettingsView,
) -> Option<SemanticSearchSettingsAction> {
    let mut action = None;
    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
        ui.label(
            egui::RichText::new("Semantic Search")
                .text_style(egui::TextStyle::Body)
                .color(crate::ui::theme::TEXT),
        );
        let (status, detail) = semantic_search_status_copy(&semantic_search.status);
        ui.label(
            egui::RichText::new(status)
                .text_style(egui::TextStyle::Small)
                .color(crate::ui::theme::TEXT_MUTED),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(detail)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::ui::theme::TEXT_SUBTLE),
            )
            .wrap(),
        );
        ui.add_space(crate::ui::theme::SPACE_2);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
            match &semantic_search.status {
                SemanticSearchSettingsStatus::Installed => {
                    if ui.add(quiet_settings_button("Remove")).clicked() {
                        action = Some(SemanticSearchSettingsAction::RequestRemove);
                    }
                }
                SemanticSearchSettingsStatus::Missing => {
                    if ui.add(quiet_settings_button("Install")).clicked() {
                        action = Some(SemanticSearchSettingsAction::Install);
                    }
                }
                SemanticSearchSettingsStatus::Downloading { .. } => {
                    if ui.add(quiet_settings_button("Cancel")).clicked() {
                        action = Some(SemanticSearchSettingsAction::Cancel);
                    }
                }
                SemanticSearchSettingsStatus::Failed => {
                    if ui.add(quiet_settings_button("Retry")).clicked() {
                        action = Some(SemanticSearchSettingsAction::Retry);
                    }
                }
            }
        });
    });
    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);
    action
}

pub(super) fn semantic_search_status_copy(
    status: &SemanticSearchSettingsStatus,
) -> (String, &'static str) {
    match status {
        SemanticSearchSettingsStatus::Installed => {
            ("Installed".to_owned(), "Local search assets are ready.")
        }
        SemanticSearchSettingsStatus::Missing => (
            "Not installed".to_owned(),
            "Install local search assets to find references by description.",
        ),
        SemanticSearchSettingsStatus::Downloading { percent } => (
            format!("Downloading {percent}%"),
            "Search assets are downloading in the background.",
        ),
        SemanticSearchSettingsStatus::Failed => (
            "Install failed".to_owned(),
            "Try again. If it still fails, keep SnappyRef in a writable folder and check network access.",
        ),
    }
}

fn quiet_settings_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Small)
            .color(crate::ui::theme::TEXT),
    )
    .fill(crate::ui::theme::SURFACE_ELEVATED)
    .stroke(crate::ui::theme::stroke_subtle())
    .corner_radius(crate::ui::theme::RADIUS_INLINE)
}

fn paint_setting_row(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    let mut changed = false;
    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Body)
                .color(crate::ui::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(detail)
                .text_style(egui::TextStyle::Small)
                .color(crate::ui::theme::TEXT_SUBTLE),
        );
        ui.add_space(crate::ui::theme::SPACE_2);
        changed |= add_control(ui);
    });
    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);
    changed
}

fn paint_setting_divider(ui: &mut egui::Ui) {
    let available = ui.available_width();
    let pad = crate::ui::theme::SPACE_3;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, 1.0), egui::Sense::hover());
    let from = egui::pos2(rect.left() - pad, rect.center().y);
    let to = egui::pos2(rect.right() + pad, rect.center().y);
    ui.painter()
        .line_segment([from, to], crate::ui::theme::stroke_subtle());
}

fn pointer_mode_detail(mode: crate::data::PointerMode) -> &'static str {
    match mode {
        crate::data::PointerMode::Mouse => "Scroll wheel zooms. Hold Alt to pan.",
        crate::data::PointerMode::Trackpad => "Pinch to zoom, two-finger swipe to pan.",
    }
}

fn paint_pointer_mode_control(ui: &mut egui::Ui, mode: &mut crate::data::PointerMode) -> bool {
    let before = *mode;
    let segments: [(&str, crate::data::PointerMode); 2] = [
        ("Mouse", crate::data::PointerMode::Mouse),
        ("Trackpad", crate::data::PointerMode::Trackpad),
    ];

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
        for (label, value) in segments {
            let selected = *mode == value;
            if paint_inline_choice(ui, label, selected) {
                *mode = value;
            }
        }
    });

    *mode != before
}

fn paint_background_mode_control(
    ui: &mut egui::Ui,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut background = canvas.background();
    let before = background;

    let segments: [(&str, crate::data::CanvasBackground); 3] = [
        ("None", crate::data::CanvasBackground::None),
        ("Grid", crate::data::CanvasBackground::Grid),
        ("Dots", crate::data::CanvasBackground::Dots),
    ];

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
        for (label, value) in segments {
            let selected = background == value;
            if paint_inline_choice(ui, label, selected) {
                background = value;
            }
        }
    });

    if background != before {
        canvas.set_background(background);
        true
    } else {
        false
    }
}

fn paint_inline_choice(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    paint_inline_choice_response(ui, label, selected).clicked()
}

fn paint_inline_choice_response(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let text_color = if selected {
        crate::ui::theme::TEXT
    } else {
        crate::ui::theme::TEXT_MUTED
    };
    let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let pad = egui::vec2(
        crate::ui::theme::QUIET_INLINE_PAD_X,
        crate::ui::theme::QUIET_INLINE_PAD_Y,
    );
    let size = egui::vec2(
        (galley.size().x + pad.x * 2.0).max(crate::ui::theme::QUIET_INLINE_MIN_WIDTH),
        crate::ui::theme::QUIET_INLINE_HEIGHT,
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let fill = if selected {
        crate::ui::theme::QUIET_INLINE_ACTIVE_FILL
    } else if response.hovered() {
        crate::ui::theme::QUIET_INLINE_HOVER_FILL
    } else {
        egui::Color32::TRANSPARENT
    };
    if selected || response.hovered() {
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_INLINE),
            fill,
            if selected {
                crate::ui::theme::stroke_subtle()
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        crate::ui::layout::centered_pos(rect, galley.size()),
        galley,
        text_color,
    );
    response
}

fn paint_number_text_field(
    ui: &mut egui::Ui,
    id_source: &'static str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &'static str,
) -> bool {
    let id = ui.make_persistent_id(("settings-number-field", id_source));
    let has_focus = ui.memory(|memory| memory.has_focus(id));
    let mut text = ui
        .data(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| format!("{value:.0}"));
    if !has_focus {
        text = format!("{value:.0}");
    }

    let mut changed = false;
    let frame = egui::Frame::new()
        .fill(crate::ui::theme::QUIET_INLINE_ACTIVE_FILL)
        .stroke(crate::ui::theme::stroke_subtle())
        .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_INLINE))
        .inner_margin(egui::Margin {
            left: crate::ui::theme::QUIET_INLINE_PAD_X as i8,
            right: crate::ui::theme::QUIET_INLINE_PAD_X as i8,
            top: crate::ui::theme::SPACE_1 as i8,
            bottom: crate::ui::theme::SPACE_1 as i8,
        });
    let response = frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
                let response = ui
                    .scope(|ui| {
                        ui.visuals_mut().selection.bg_fill = crate::ui::theme::ACCENT;
                        ui.visuals_mut().selection.stroke =
                            egui::Stroke::new(1.0, crate::ui::theme::BACKGROUND);
                        ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .id(id)
                                .font(egui::TextStyle::Button)
                                .text_color(crate::ui::theme::TEXT)
                                .frame(egui::Frame::NONE)
                                .desired_width(48.0),
                        )
                    })
                    .inner;
                ui.label(
                    egui::RichText::new(suffix)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::ui::theme::TEXT_MUTED),
                );
                response
            })
            .inner
        })
        .inner;

    if response.changed() {
        if let Ok(parsed) = text.trim().parse::<f32>() {
            let clamped = parsed.clamp(*range.start(), *range.end());
            if (*value - clamped).abs() > f32::EPSILON {
                *value = clamped;
                changed = true;
            }
        }
    }

    let commit_invalid = response.lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter) || input.pointer.any_click())
        && text.trim().parse::<f32>().is_err();
    if commit_invalid {
        text = format!("{value:.0}");
    }
    ui.data_mut(|data| data.insert_temp(id, text));

    changed
}

fn paint_switch(ui: &mut egui::Ui, value: &mut bool) -> egui::Response {
    let label = if *value { "On" } else { "Off" };
    let before = *value;
    let mut response = paint_inline_choice_response(ui, label, before);
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response
}
