use eframe::egui;

use super::AboutUpdatesView;

pub(super) struct AboutUpdatesOutcome {
    pub(super) update_action_clicked: bool,
}

pub(super) fn paint_about_updates_section(
    ui: &mut egui::Ui,
    about: &AboutUpdatesView,
) -> AboutUpdatesOutcome {
    let mut update_action_clicked = false;

    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
        ui.label(
            egui::RichText::new(format!("Version {}", about.current_version))
                .text_style(egui::TextStyle::Body)
                .color(crate::ui::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(format!(
                "Last checked: {}",
                format_last_checked(about.last_checked_unix_millis),
            ))
            .text_style(egui::TextStyle::Small)
            .color(crate::ui::theme::TEXT_SUBTLE),
        );
        ui.add_space(crate::ui::theme::SPACE_2);
        if let Some(status) = &about.update_status {
            ui.label(
                egui::RichText::new(status)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::ui::theme::TEXT_MUTED),
            );
            ui.add_space(crate::ui::theme::SPACE_2);
        }
        let button_width = ui.available_width();
        let text_color = if about.update_button_primary {
            crate::ui::theme::BACKGROUND
        } else {
            crate::ui::theme::TEXT
        };
        let mut update_button = egui::Button::new(
            egui::RichText::new(&about.update_button_label)
                .text_style(egui::TextStyle::Small)
                .color(text_color),
        )
        .corner_radius(crate::ui::theme::RADIUS_INLINE);
        if about.update_button_primary {
            update_button = update_button
                .fill(crate::ui::theme::ACCENT)
                .stroke(egui::Stroke::NONE);
        }
        if ui
            .add_enabled_ui(about.update_button_enabled, |ui| {
                ui.add_sized([button_width, 0.0], update_button)
            })
            .inner
            .clicked()
        {
            update_action_clicked = true;
        }
    });
    ui.add_space(crate::ui::theme::SPACE_2 + 2.0);

    AboutUpdatesOutcome {
        update_action_clicked,
    }
}

fn format_last_checked(unix_millis: Option<u64>) -> String {
    let Some(millis) = unix_millis else {
        return "never".to_owned();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(millis);
    let elapsed_millis = now.saturating_sub(millis);
    let elapsed_seconds = elapsed_millis / 1_000;
    if elapsed_seconds < 60 {
        "just now".to_owned()
    } else if elapsed_seconds < 60 * 60 {
        let minutes = elapsed_seconds / 60;
        if minutes == 1 {
            "1 minute ago".to_owned()
        } else {
            format!("{minutes} minutes ago")
        }
    } else if elapsed_seconds < 60 * 60 * 24 {
        let hours = elapsed_seconds / 3_600;
        if hours == 1 {
            "1 hour ago".to_owned()
        } else {
            format!("{hours} hours ago")
        }
    } else {
        let days = elapsed_seconds / 86_400;
        if days == 1 {
            "1 day ago".to_owned()
        } else {
            format!("{days} days ago")
        }
    }
}
