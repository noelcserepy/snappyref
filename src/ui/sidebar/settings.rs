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
        "Placement gap",
        "Spacing used when placing and arranging placements.",
        |ui| {
            paint_number_text_field(
                ui,
                "placement-gap",
                &mut settings.placement_gap,
                0.0..=128.0,
                "units",
            )
        },
    );

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
    .fill(crate::ui::theme::QUIET_INLINE_ACTIVE_FILL)
    .stroke(egui::Stroke::NONE)
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

/// Settings rows separate with spacing alone; no rules between rows.
fn paint_setting_divider(ui: &mut egui::Ui) {
    ui.add_space(crate::ui::theme::SPACE_1);
}

fn pointer_mode_detail(mode: crate::data::PointerMode) -> &'static str {
    match mode {
        crate::data::PointerMode::Mouse => "Scroll wheel zooms. Hold Alt to pan.",
        crate::data::PointerMode::Trackpad => "Pinch to zoom, two-finger swipe to pan.",
    }
}

fn paint_pointer_mode_control(ui: &mut egui::Ui, mode: &mut crate::data::PointerMode) -> bool {
    let segments: [(&str, crate::data::PointerMode); 2] = [
        ("Mouse", crate::data::PointerMode::Mouse),
        ("Trackpad", crate::data::PointerMode::Trackpad),
    ];
    paint_inline_choice_group(ui, "pointer-mode", mode, &segments)
}

fn paint_background_mode_control(
    ui: &mut egui::Ui,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut background = canvas.background();

    let segments: [(&str, crate::data::CanvasBackground); 3] = [
        ("None", crate::data::CanvasBackground::None),
        ("Grid", crate::data::CanvasBackground::Grid),
        ("Dots", crate::data::CanvasBackground::Dots),
    ];

    if paint_inline_choice_group(ui, "canvas-background", &mut background, &segments) {
        canvas.set_background(background);
        true
    } else {
        false
    }
}

/// Quiet segmented choices for read-heavy settings rows.
///
/// Only the selected value gets a persistent mark: a sliding accent underline.
/// Idle choices should read as available text, not boxed pills. Keep this
/// pattern inside sidebar/settings surfaces; canvas toolbars need more
/// explicit targets.
fn paint_inline_choice_group<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    id_source: &'static str,
    value: &mut T,
    segments: &[(&str, T)],
) -> bool {
    let before = *value;
    let mut selected_rect = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
        for (label, segment_value) in segments {
            let selected = *value == *segment_value;
            let response = paint_inline_choice_response(ui, label, selected);
            if selected {
                selected_rect = Some(response.rect);
            }
            if response.clicked() {
                *value = *segment_value;
            }
        }
    });
    if let Some(rect) = selected_rect {
        let center_x = crate::ui::motion::slide_indicator(
            ui.ctx(),
            egui::Id::new(("inline-choice-underline-x", id_source)),
            rect.center().x,
        );
        let half_width = crate::ui::motion::slide_indicator(
            ui.ctx(),
            egui::Id::new(("inline-choice-underline-w", id_source)),
            (rect.width() - crate::ui::theme::QUIET_INLINE_PAD_X) * 0.5,
        );
        paint_inline_choice_underline(ui, center_x, half_width, rect.bottom());
    }
    *value != before
}

fn paint_inline_choice_underline(ui: &egui::Ui, center_x: f32, half_width: f32, bottom: f32) {
    let y = bottom - 1.0;
    ui.painter().line_segment(
        [
            egui::pos2(center_x - half_width, y),
            egui::pos2(center_x + half_width, y),
        ],
        egui::Stroke::new(2.0_f32, crate::ui::theme::ACCENT),
    );
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
    let hover = crate::ui::motion::hover_amount(
        ui.ctx(),
        response.id.with("inline-choice-hover"),
        response.hovered() && !selected,
    );
    let press = crate::ui::motion::press_amount(
        ui.ctx(),
        response.id.with("inline-choice-press"),
        response.is_pointer_button_down_on(),
    );
    let acquire = crate::ui::motion::acquire_amount(
        ui.ctx(),
        response.id.with("inline-choice-acquire"),
        selected,
    );
    let paint_rect = crate::ui::motion::pressed_rect(rect, press);
    if !selected && (hover > 0.0 || press > 0.0) {
        ui.painter().rect(
            paint_rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_INLINE),
            crate::ui::motion::mix(
                crate::ui::theme::QUIET_INLINE_HOVER_FILL.gamma_multiply(hover),
                crate::ui::theme::SURFACE_PRESSED,
                press * 0.65,
            ),
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    } else if selected && acquire < 1.0 {
        ui.painter().rect(
            crate::ui::motion::pressed_rect(rect, 1.0 - acquire),
            egui::CornerRadius::same(crate::ui::theme::RADIUS_INLINE),
            crate::ui::theme::QUIET_INLINE_ACTIVE_FILL.gamma_multiply(1.0 - acquire),
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        rect.center() - galley.size() * 0.5 + egui::vec2(0.0, press * 0.5),
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
                            egui::Stroke::new(1.0_f32, crate::ui::theme::BACKGROUND);
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

    if response.changed()
        && let Ok(parsed) = text.trim().parse::<f32>()
    {
        let clamped = parsed.clamp(*range.start(), *range.end());
        if (*value - clamped).abs() > f32::EPSILON {
            *value = clamped;
            changed = true;
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
    if before {
        let rect = response.rect;
        paint_inline_choice_underline(
            ui,
            rect.center().x,
            (rect.width() - crate::ui::theme::QUIET_INLINE_PAD_X) * 0.5,
            rect.bottom(),
        );
    }
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response
}

#[cfg(test)]
mod release_tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn numeric_settings_clamp_bounds_and_restore_invalid_text_on_blur() {
        for upper in [180.0, 128.0] {
            for (text, expected) in [
                ("0", 0.0),
                ("999", upper),
                ("-4", 0.0),
                ("", 15.0),
                ("abc", 15.0),
            ] {
                let mut harness = Harness::builder()
                    .with_size(egui::vec2(400.0, 100.0))
                    .build_ui_state(
                        move |ui, value| {
                            paint_number_text_field(
                                ui,
                                "release-test",
                                value,
                                0.0..=upper,
                                "units",
                            );
                            ui.button("Elsewhere").clicked();
                        },
                        15.0,
                    );
                harness
                    .get_by_role(egui::accesskit::Role::TextInput)
                    .click();
                harness.step();
                harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
                harness.step();
                harness.key_press(egui::Key::Backspace);
                harness.step();
                if !text.is_empty() {
                    harness.event(egui::Event::Text(text.into()));
                    harness.step();
                }
                harness.get_by_label("Elsewhere").click();
                harness.step();
                assert_eq!(
                    *harness.state(),
                    expected,
                    "input {text:?}, maximum {upper}"
                );
                assert!(!harness.ctx.egui_wants_keyboard_input());
            }
        }
    }
}
