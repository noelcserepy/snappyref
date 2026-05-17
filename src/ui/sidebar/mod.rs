use eframe::egui;

mod about_updates;
mod boards;
mod controls;
mod settings;
mod shortcuts;

use about_updates::paint_about_updates_section;
use boards::paint_sidebar_boards;
use controls::{paint_sidebar_section_header, paint_sidebar_subcard, paint_sidebar_tabs};
use settings::paint_settings_section;
use shortcuts::paint_supported_shortcuts_section;

pub(crate) use controls::show_sidebar_hint;
pub(crate) const SIDEBAR_WIDTH: f32 = 268.0;
pub(crate) const SIDEBAR_TOP_MARGIN: f32 = 40.0;
pub(crate) const SIDEBAR_BOTTOM_MARGIN: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarTab {
    Boards,
    Options,
}

impl Default for SidebarTab {
    fn default() -> Self {
        SidebarTab::Options
    }
}

pub(crate) struct SidebarRenderResult {
    pub settings_changed: bool,
    pub interactive_rect: Option<egui::Rect>,
    pub board_action: Option<SidebarBoardAction>,
    /// True when the Settings Tab update action was activated this frame.
    pub update_action_clicked: bool,
    pub semantic_search_action: Option<SemanticSearchSettingsAction>,
}

/// Snapshot of the About / Updates section state. The sidebar reads but
/// does not mutate any of these; the app owns the data.
#[derive(Clone, Debug)]
pub(crate) struct AboutUpdatesView {
    /// Running app version, normally `env!("CARGO_PKG_VERSION")`.
    pub current_version: String,
    /// Unix-epoch milliseconds of the last completed check. `None`
    /// renders as "never".
    pub last_checked_unix_millis: Option<u64>,
    pub update_button_label: String,
    pub update_button_enabled: bool,
    pub update_button_primary: bool,
    pub update_status: Option<String>,
    pub semantic_search: SemanticSearchSettingsView,
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticSearchSettingsView {
    pub status: SemanticSearchSettingsStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SemanticSearchSettingsStatus {
    Installed,
    Missing,
    Downloading { percent: u8 },
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SemanticSearchSettingsAction {
    Install,
    Retry,
    Cancel,
    RequestRemove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarOptionsSection {
    AboutUpdates,
    WorkspaceSettings,
    Shortcuts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SidebarBoardAction {
    CreateBoard,
    SwitchBoard(i64),
    RenameBoard(i64, String),
    RequestDeleteBoard(i64),
}

pub(crate) fn sidebar_rect(screen_rect: egui::Rect) -> egui::Rect {
    let top = screen_rect.top() + SIDEBAR_TOP_MARGIN;
    let bottom = (screen_rect.bottom() - SIDEBAR_BOTTOM_MARGIN).max(top);
    egui::Rect::from_min_max(
        egui::pos2(screen_rect.left(), top),
        egui::pos2(screen_rect.left() + SIDEBAR_WIDTH, bottom),
    )
}

pub(crate) fn block_sidebar_input(ctx: &egui::Context, sidebar_rect: egui::Rect) {
    if sidebar_rect.height() <= 0.0 || sidebar_rect.width() <= 0.0 {
        return;
    }

    egui::Area::new(egui::Id::new("sidebar_blocker"))
        .order(egui::Order::Middle)
        .fixed_pos(sidebar_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(sidebar_rect.size());
            let local_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, sidebar_rect.size());
            ui.allocate_rect(local_rect, egui::Sense::hover());
        });
}

pub(crate) fn close_sidebar_on_outside_click(
    ctx: &egui::Context,
    sidebar_rect: egui::Rect,
    sidebar_open_state: &mut bool,
) {
    let clicked_outside = ctx.input(|input| {
        should_close_sidebar(
            input.pointer.any_click(),
            input.pointer.interact_pos(),
            sidebar_rect,
        )
    });

    if clicked_outside {
        *sidebar_open_state = false;
    }
}

pub(crate) fn should_close_sidebar(
    pointer_clicked: bool,
    pointer_pos: Option<egui::Pos2>,
    sidebar_rect: egui::Rect,
) -> bool {
    pointer_clicked && pointer_pos.is_some_and(|pos| !sidebar_rect.contains(pos))
}

pub(crate) fn show_sidebar(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
    input_enabled: bool,
    sidebar_tab: &mut SidebarTab,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    boards: &[crate::data::BoardSummary],
    active_board_id: i64,
    forced_board_hover_index: Option<usize>,
    _update_state: &crate::updater::UpdateState,
    about_updates: AboutUpdatesView,
) -> SidebarRenderResult {
    if !sidebar_open {
        return SidebarRenderResult {
            settings_changed: false,
            interactive_rect: None,
            board_action: None,
            update_action_clicked: false,
            semantic_search_action: None,
        };
    }

    let mut settings_changed = false;
    let mut board_action = None;
    let mut update_action_clicked = false;
    let mut semantic_search_action = None;

    let top = screen_rect.top() + SIDEBAR_TOP_MARGIN;
    let bottom = (screen_rect.bottom() - SIDEBAR_BOTTOM_MARGIN).max(top);
    let sidebar_rect = egui::Rect::from_min_max(
        egui::pos2(screen_rect.left(), top),
        egui::pos2(screen_rect.left() + SIDEBAR_WIDTH, bottom),
    );

    let area_response = egui::Area::new(egui::Id::new("sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(sidebar_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(sidebar_rect.size());
            let shape_rect = sidebar_rect;
            let radius = egui::CornerRadius {
                nw: 0,
                ne: crate::ui::theme::RADIUS_PANEL,
                sw: 0,
                se: crate::ui::theme::RADIUS_PANEL,
            };
            ui.painter().rect(
                shape_rect,
                radius,
                crate::ui::theme::SURFACE,
                crate::ui::theme::stroke_subtle(),
                egui::StrokeKind::Inside,
            );

            let inner_rect = shape_rect;
            let ui_builder = egui::UiBuilder::new().max_rect(inner_rect);
            ui.scope_builder(ui_builder, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let mut paint_contents = |ui: &mut egui::Ui| {
                    paint_sidebar_tabs(ui, sidebar_tab);
                    ui.add_space(crate::ui::theme::SPACE_2);
                    match *sidebar_tab {
                        SidebarTab::Boards => {
                            board_action = paint_sidebar_boards(
                                ui,
                                boards,
                                active_board_id,
                                forced_board_hover_index,
                            )
                        }
                        SidebarTab::Options => {
                            let options_outcome =
                                paint_sidebar_options(ui, settings, canvas, &about_updates);
                            settings_changed |= options_outcome.settings_changed;
                            update_action_clicked |= options_outcome.update_action_clicked;
                            if semantic_search_action.is_none() {
                                semantic_search_action = options_outcome.semantic_search_action;
                            }
                        }
                    }
                };
                if input_enabled {
                    paint_contents(ui);
                } else {
                    ui.add_enabled_ui(false, |ui| paint_contents(ui));
                }
            });
        });

    SidebarRenderResult {
        settings_changed,
        interactive_rect: Some(area_response.response.rect),
        board_action,
        update_action_clicked,
        semantic_search_action,
    }
}

struct SidebarOptionsOutcome {
    settings_changed: bool,
    update_action_clicked: bool,
    semantic_search_action: Option<SemanticSearchSettingsAction>,
}

fn paint_sidebar_options(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    about_updates: &AboutUpdatesView,
) -> SidebarOptionsOutcome {
    let mut changed = false;
    let mut update_action_clicked = false;
    let mut semantic_search_action = None;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        egui::ScrollArea::vertical()
            .id_salt("sidebar_options_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.add_space(crate::ui::theme::SPACE_1);
                for (index, section) in
                    sidebar_options_section_order(&about_updates.semantic_search.status)
                        .into_iter()
                        .enumerate()
                {
                    if index > 0 {
                        ui.add_space(match section {
                            SidebarOptionsSection::Shortcuts => crate::ui::theme::SPACE_6,
                            _ => crate::ui::theme::SPACE_4,
                        });
                    }
                    match section {
                        SidebarOptionsSection::AboutUpdates => {
                            paint_sidebar_subcard(ui, |ui| {
                                let about_outcome = paint_about_updates_section(ui, about_updates);
                                update_action_clicked |= about_outcome.update_action_clicked;
                            });
                        }
                        SidebarOptionsSection::WorkspaceSettings => {
                            paint_sidebar_subcard(ui, |ui| {
                                let settings_outcome = paint_settings_section(
                                    ui,
                                    settings,
                                    canvas,
                                    &about_updates.semantic_search,
                                );
                                changed |= settings_outcome.changed;
                                semantic_search_action = settings_outcome.semantic_search_action;
                            });
                        }
                        SidebarOptionsSection::Shortcuts => {
                            paint_sidebar_section_header(ui, "Shortcuts", None);
                            paint_supported_shortcuts_section(ui);
                        }
                    }
                }
                ui.add_space(crate::ui::theme::SPACE_4);
            });
    });
    SidebarOptionsOutcome {
        settings_changed: changed,
        update_action_clicked,
        semantic_search_action,
    }
}

fn sidebar_options_section_order(
    semantic_search_status: &SemanticSearchSettingsStatus,
) -> Vec<SidebarOptionsSection> {
    let _ = semantic_search_status;
    vec![
        SidebarOptionsSection::AboutUpdates,
        SidebarOptionsSection::WorkspaceSettings,
        SidebarOptionsSection::Shortcuts,
    ]
}

#[cfg(test)]
mod tests {
    use super::boards::paint_board_row;
    use super::settings::semantic_search_status_copy;
    use super::shortcuts::{display_shortcut_label, shortcut_label_uses_small_symbol_glyphs};
    use super::{
        SemanticSearchSettingsStatus, SidebarOptionsSection, sidebar_options_section_order,
    };
    use eframe::egui;

    #[test]
    fn display_shortcut_label_spaces_compact_modifier_chords() {
        assert_eq!(display_shortcut_label("⌘B"), "⌘ B");
        assert_eq!(display_shortcut_label("⇧⌘L"), "⇧ ⌘ L");
        assert_eq!(display_shortcut_label("Ctrl+B"), "Ctrl B");
        assert_eq!(display_shortcut_label("Ctrl+Shift+L"), "Ctrl Shift L");
        assert_eq!(display_shortcut_label("C + Drag"), "C + Drag");
        assert_eq!(display_shortcut_label("R + Drag"), "R + Drag");
    }

    #[test]
    fn shortcut_label_detects_visually_small_symbol_glyphs() {
        assert!(shortcut_label_uses_small_symbol_glyphs("⇧ ⌘ L"));
        assert!(shortcut_label_uses_small_symbol_glyphs("↵"));
        assert!(!shortcut_label_uses_small_symbol_glyphs("⌘ B"));
    }

    #[test]
    fn semantic_search_settings_status_copy_covers_lifecycle_states() {
        assert_eq!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Missing),
            (
                "Not installed".to_owned(),
                "Install local search assets to find references by description."
            )
        );
        assert_eq!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Downloading { percent: 37 })
                .0,
            "Downloading 37%"
        );
        assert_eq!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Failed).0,
            "Install failed"
        );
        assert!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Failed)
                .1
                .contains("writable folder")
        );
        assert_eq!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Installed).0,
            "Installed"
        );
    }

    #[test]
    fn sidebar_options_keep_about_updates_at_top_when_semantic_search_is_installed() {
        assert_eq!(
            sidebar_options_section_order(&SemanticSearchSettingsStatus::Installed),
            vec![
                SidebarOptionsSection::AboutUpdates,
                SidebarOptionsSection::WorkspaceSettings,
                SidebarOptionsSection::Shortcuts,
            ]
        );
    }

    #[test]
    fn sidebar_options_keep_about_updates_at_top_when_semantic_search_is_missing() {
        assert_eq!(
            sidebar_options_section_order(&SemanticSearchSettingsStatus::Missing),
            vec![
                SidebarOptionsSection::AboutUpdates,
                SidebarOptionsSection::WorkspaceSettings,
                SidebarOptionsSection::Shortcuts,
            ]
        );
    }

    #[test]
    fn board_row_rename_mode_consumes_full_row_height() {
        let ctx = egui::Context::default();
        let mut cursor_after_row = 0.0;

        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(268.0, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.set_width(240.0);
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let name_edit_id = ui.make_persistent_id(("board-name-edit", 1_i64));
                ui.memory_mut(|memory| memory.request_focus(name_edit_id));

                paint_board_row(ui, 1, "My Board", "96 images", true, Some("⌘1"), false);
                cursor_after_row = ui.cursor().min.y;
            },
        );

        assert_eq!(cursor_after_row, 48.0);
    }
}
