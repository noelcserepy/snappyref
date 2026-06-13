use eframe::egui;

use crate::app::{SUPPORTED_SHORTCUTS, SupportedShortcut};

pub(super) fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    let pad = crate::ui::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        let (core_shortcuts, layout_shortcuts) = SUPPORTED_SHORTCUTS.split_at(10);
        paint_shortcut_group(ui, "Core actions", core_shortcuts);
        ui.add_space(crate::ui::theme::SPACE_3);
        paint_shortcut_group(ui, "Layout + history", layout_shortcuts);
    });
}

fn paint_shortcut_group(ui: &mut egui::Ui, title: &str, shortcuts: &[SupportedShortcut]) {
    ui.label(crate::ui::theme::editorial_caption(title));
    ui.add_space(crate::ui::theme::SPACE_1);
    for shortcut in shortcuts.iter() {
        paint_shortcut_row(ui, shortcut);
    }
}

fn paint_shortcut_row(ui: &mut egui::Ui, shortcut: &SupportedShortcut) {
    #[cfg(target_os = "macos")]
    let keys = shortcut.mac;
    #[cfg(not(target_os = "macos"))]
    let keys = shortcut.windows;

    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::ui::theme::SPACE_2;
        ui.with_layout(crate::ui::layout::row_end_center_y_layout(), |ui| {
            paint_shortcut_pill(ui, keys);
            ui.with_layout(crate::ui::layout::row_center_y_layout(), |ui| {
                ui.label(
                    egui::RichText::new(shortcut.action)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::ui::theme::TEXT),
                );
            });
        });
    });
    ui.add_space(3.0);
}

/// Shortcut keys read as quiet tabular metadata: bare monospace text with
/// no pill chrome. The key column stays right-aligned for scanning.
fn paint_shortcut_pill(ui: &mut egui::Ui, keys: &str) {
    let keys = display_shortcut_label(keys);
    let mut font = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    if shortcut_label_uses_small_symbol_glyphs(&keys) {
        font.size += 3.0;
    }
    let galley = ui
        .painter()
        .layout_no_wrap(keys, font, crate::ui::theme::TEXT_MUTED);
    let pad = egui::vec2(0.0, 1.0);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .galley(rect.min + pad, galley, crate::ui::theme::TEXT_MUTED);
}

pub(super) fn sidebar_shortcut_label() -> &'static str {
    platform_shortcut_label("⌘B", "Ctrl+B")
}

pub(super) fn platform_command_modifier_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Cmd"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl"
    }
}

#[cfg(target_os = "macos")]
fn platform_shortcut_label(mac: &'static str, _windows: &'static str) -> &'static str {
    mac
}

#[cfg(not(target_os = "macos"))]
fn platform_shortcut_label(_mac: &'static str, windows: &'static str) -> &'static str {
    windows
}

pub(super) fn display_shortcut_label(keys: &str) -> String {
    if keys.contains(" + ") {
        return keys.to_owned();
    }

    if keys.contains('+') {
        return keys
            .split('+')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }

    let mut label = String::new();
    let mut previous_was_modifier = false;

    for ch in keys.chars() {
        let is_modifier = matches!(ch, '⌘' | '⇧' | '⌥' | '⌃');
        if !label.is_empty() && (previous_was_modifier || is_modifier) {
            label.push(' ');
        }
        label.push(ch);
        previous_was_modifier = is_modifier;
    }

    label
}

pub(super) fn shortcut_label_uses_small_symbol_glyphs(keys: &str) -> bool {
    keys.chars().any(|ch| matches!(ch, '⇧' | '↵'))
}
