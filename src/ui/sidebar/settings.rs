//! The Settings tab: a page bar over General and Keys. General ends with
//! Semantic Search and About, the two things the app installs or updates.

use eframe::egui::{self, Align2, FontId, RichText};

use super::icons::{self, Icon};
use super::{
    INNER_WIDTH, SemanticSearchSettingsAction, SemanticSearchSettingsStatus, SidebarRenderResult,
    about,
};
use crate::app::TOGGLE_ALWAYS_ON_TOP;
use crate::data::{CanvasBackground, PointerMode};
use crate::ui::{card, theme};

/// Two controls side by side on the card's inner column.
const HALF: f32 = (INNER_WIDTH - card::ITEM_SPACING.x) * 0.5;
/// A bed's cells share its width after the 2px margin and gaps.
fn bed_cell(count: f32) -> f32 {
    (INNER_WIDTH - 4.0 - 2.0 * (count - 1.0)) / count
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Page {
    #[default]
    General,
    Keys,
}

const PAGES: [(Page, &str, Icon); 2] = [
    (Page::General, "General", Icon::General),
    (Page::Keys, "Keys", Icon::Keys),
];

pub(super) fn show(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    about: &about::AboutView,
    semantic_search: &SemanticSearchSettingsStatus,
    result: &mut SidebarRenderResult,
) {
    let page_id = egui::Id::new("sidebar-settings-page");
    let mut page: Page = ui.data(|d| d.get_temp(page_id)).unwrap_or_default();
    let about = about::status(about);
    let (search, search_action) = search_status(semantic_search);
    let open = PAGES.iter().position(|(p, ..)| *p == page).unwrap_or(0);
    // A tab holding a step that waits on the user, like an update, wears a dot.
    let attention = |p: Page| match p {
        Page::General => about.primary || search.primary,
        Page::Keys => false,
    };
    let names = PAGES.map(|(_, name, _)| name);
    let clicked = super::column(ui, 48.0, |ui| {
        card::tabs(
            ui,
            egui::vec2(INNER_WIDTH, 48.0),
            &names,
            open,
            |p, i, r, ink| {
                let (page, name, icon) = PAGES[i];
                icons::paint(p, r.center() - egui::vec2(0.0, 8.0), icon, ink);
                p.text(
                    r.center() + egui::vec2(0.0, 10.0),
                    Align2::CENTER_CENTER,
                    name,
                    FontId::proportional(theme::TYPE_CHIP),
                    ink,
                );
                if i != open && attention(page) {
                    p.circle_filled(r.right_top() + egui::vec2(-9.0, 9.0), 3.0, theme::ACCENT);
                }
            },
        )
    });
    if let Some(i) = clicked {
        page = PAGES[i].0;
        ui.data_mut(|d| d.insert_temp(page_id, page));
    }
    super::scroll(ui, ("sidebar_settings", page), |ui| match page {
        Page::General => {
            result.settings_changed |= general(ui, settings, canvas);
            card::section(ui, "Semantic search");
            if status_row(ui, &search) {
                result.semantic_search_action = Some(search_action);
            }
            card::section(ui, "About");
            result.update_action_clicked |= status_row(ui, &about);
        }
        Page::Keys => super::keys::show(ui),
    });
}

/// Returns true when a setting changed.
fn general(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;
    card::section(ui, "Pointer");
    let pointer = [
        (PointerMode::Mouse, "Mouse", Icon::Mouse),
        (PointerMode::Trackpad, "Trackpad", Icon::Trackpad),
    ];
    if let Some(mode) = choice(ui, &pointer, settings.pointer_mode) {
        settings.pointer_mode = mode;
        changed = true;
    }
    detail(ui, pointer_mode_detail(settings.pointer_mode));

    card::section(ui, "Canvas");
    let backgrounds = [
        (CanvasBackground::None, "None", Icon::NoGuide),
        (CanvasBackground::Grid, "Grid", Icon::Grid),
        (CanvasBackground::Dots, "Dots", Icon::Dots),
    ];
    if let Some(background) = choice(ui, &backgrounds, canvas.background()) {
        canvas.set_background(background);
        changed = true;
    }
    ui.horizontal(|ui| {
        changed |= number(
            ui,
            Number::ROTATION_SNAP,
            &mut settings.rotation_snap_degrees,
        );
        changed |= number(ui, Number::PLACEMENT_GAP, &mut settings.placement_gap);
    });
    let fill = settings.default_textbox_background;
    if card::switch(
        ui,
        "Fill new textboxes",
        INNER_WIDTH,
        fill,
        true,
        None,
        |p, c, ink| icons::paint(p, c, Icon::Textbox, ink),
    )
    .clicked()
    {
        settings.default_textbox_background = !fill;
        changed = true;
    }

    card::section(ui, "Window");
    let on_top = settings.always_on_top;
    if card::switch(
        ui,
        "Always on top",
        INNER_WIDTH,
        on_top,
        true,
        Some(TOGGLE_ALWAYS_ON_TOP),
        |p, c, ink| icons::paint(p, c, Icon::OnTop, ink),
    )
    .clicked()
    {
        settings.always_on_top = !on_top;
        changed = true;
    }
    changed
}

/// A wrapped line of quiet text under the control it explains.
fn detail(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(theme::TYPE_DETAIL)
            .color(theme::TEXT_SUBTLE),
    );
}

fn pointer_mode_detail(mode: PointerMode) -> &'static str {
    match mode {
        PointerMode::Mouse => "Scroll wheel zooms. Hold Alt to pan.",
        PointerMode::Trackpad => "Pinch to zoom, two-finger swipe to pan.",
    }
}

/// A bed of icon-and-label cells across the column. Returns the value
/// clicked this frame.
fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    options: &[(T, &str, Icon)],
    value: T,
) -> Option<T> {
    let width = bed_cell(options.len() as f32);
    let mut clicked = None;
    card::bed(ui, |ui| {
        for &(option, name, icon) in options {
            let selected = option == value;
            let r = card::cell(
                ui,
                name,
                egui::vec2(width, theme::CONTROL_HEIGHT),
                selected,
                true,
                theme::SURFACE_PRESSED,
            );
            let ink = card::ink(selected, true, false);
            let font = FontId::proportional(theme::TYPE_CONTROL);
            let galley = ui.painter().layout_no_wrap(name.to_owned(), font, ink);
            let left = r.rect.center().x - (14.0 + 6.0 + galley.size().x) * 0.5;
            icons::paint(
                ui.painter(),
                egui::pos2(left + 7.0, r.rect.center().y),
                icon,
                ink,
            );
            ui.painter().galley(
                egui::pos2(left + 20.0, r.rect.center().y - galley.size().y * 0.5),
                galley,
                ink,
            );
            if r.clicked() && !selected {
                clicked = Some(option);
            }
        }
    });
    clicked
}

/// A numeric setting's well: scrub icon, short name, field, unit.
#[derive(Clone, Copy)]
struct Number {
    name: &'static str,
    short: &'static str,
    tooltip: &'static str,
    unit: &'static str,
    icon: Icon,
    max: f32,
}

impl Number {
    const ROTATION_SNAP: Number = Number {
        name: "Rotation snap",
        short: "Snap",
        tooltip: "rotation snap",
        unit: "°",
        icon: Icon::Rotation,
        max: 180.0,
    };
    const PLACEMENT_GAP: Number = Number {
        name: "Placement gap",
        short: "Gap",
        tooltip: "placement gap",
        unit: "px",
        icon: Icon::Gap,
        max: 128.0,
    };
}

/// Typed values clamp into range; text that is not a number reverts.
/// Returns true when the value changed.
fn number(ui: &mut egui::Ui, spec: Number, value: &mut f32) -> bool {
    let before = *value;
    card::well(ui, HALF, |ui| {
        let (scrubbed, _) = card::scrub(
            ui,
            (spec.name, spec.tooltip),
            *value,
            0.0,
            true,
            |p, r, ink| icons::paint(p, r.center(), spec.icon, ink),
        );
        if let Some(n) = scrubbed {
            *value = n.min(spec.max);
        }
        let muted = |text| {
            RichText::new(text)
                .size(theme::TYPE_CONTROL)
                .color(theme::TEXT_MUTED)
        };
        ui.label(muted(spec.short));
        let unit = ui
            .painter()
            .layout_no_wrap(
                spec.unit.to_owned(),
                FontId::proportional(theme::TYPE_CONTROL),
                theme::TEXT_MUTED,
            )
            .size()
            .x;
        let width = ui.available_width() - unit - ui.spacing().item_spacing.x;
        let (submitted, _) = card::field(
            ui,
            spec.name,
            &format!("{value:.0}"),
            width,
            theme::CONTROL_HEIGHT,
            true,
            theme::TYPE_CONTROL,
            egui::Align::Max,
        );
        if let Some(n) = submitted
            .and_then(|text| text.trim().parse::<f32>().ok())
            .filter(|n| n.is_finite())
        {
            *value = n.clamp(0.0, spec.max);
        }
        ui.label(muted(spec.unit));
    });
    *value != before
}

/// Something the app installs or keeps current: a [`card::status`] line in a
/// well with the next action beside it, then optional download progress and
/// a detail line.
pub(super) struct StatusRow {
    pub line: String,
    pub detail: Option<String>,
    pub action: &'static str,
    /// The action is the step the user should take next, so it draws as the
    /// gold [`card::button`].
    pub primary: bool,
    pub enabled: bool,
    /// Green when ready, gold while something wants the user or is under
    /// way, red when it failed.
    pub dot: egui::Color32,
    /// Download progress from 0 to 1.
    pub progress: Option<f32>,
}

/// Returns true when the action was clicked.
fn status_row(ui: &mut egui::Ui, row: &StatusRow) -> bool {
    let clicked = ui
        .allocate_ui_with_layout(
            egui::vec2(INNER_WIDTH, theme::CONTROL_HEIGHT),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let action = if row.primary && row.enabled {
                    card::button(ui, row.action, 0.0)
                } else {
                    card::action(ui, row.action, row.enabled)
                };
                card::well(ui, ui.available_width(), |ui| {
                    card::status(ui, row.dot, &row.line);
                });
                action.clicked()
            },
        )
        .inner;
    if let Some(fraction) = row.progress {
        card::progress(ui, INNER_WIDTH, fraction);
    }
    if let Some(text) = &row.detail {
        detail(ui, text);
    }
    clicked
}

/// The Semantic Search row and the action its button sends.
fn search_status(
    status: &SemanticSearchSettingsStatus,
) -> (StatusRow, SemanticSearchSettingsAction) {
    use SemanticSearchSettingsAction as Action;
    let row = |line: &str, detail: Option<&str>, action, primary, dot| StatusRow {
        line: line.to_owned(),
        detail: detail.map(str::to_owned),
        action,
        primary,
        enabled: true,
        dot,
        progress: None,
    };
    match status {
        SemanticSearchSettingsStatus::Installed => (
            row(
                "Ready",
                Some("Find references by describing them."),
                "Remove",
                false,
                theme::GREEN,
            ),
            Action::RequestRemove,
        ),
        SemanticSearchSettingsStatus::Missing => (
            row(
                "Not installed",
                Some("Install local assets to find references by description."),
                "Install",
                true,
                theme::TEXT_SUBTLE,
            ),
            Action::Install,
        ),
        SemanticSearchSettingsStatus::Downloading { percent } => (
            StatusRow {
                progress: Some(f32::from(*percent) / 100.0),
                ..row(
                    &format!("Downloading {percent}%"),
                    None,
                    "Cancel",
                    false,
                    theme::ACCENT,
                )
            },
            Action::Cancel,
        ),
        SemanticSearchSettingsStatus::Failed => (
            row(
                "Install failed",
                Some("Keep SnappyRef in a writable folder and check network access."),
                "Retry",
                true,
                theme::DESTRUCTIVE,
            ),
            Action::Retry,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn numeric_settings_clamp_bounds_and_restore_invalid_text_on_blur() {
        for spec in [Number::ROTATION_SNAP, Number::PLACEMENT_GAP] {
            let upper = spec.max;
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
                            number(ui, spec, value);
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
