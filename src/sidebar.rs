use eframe::egui;

use crate::app::{SUPPORTED_SHORTCUTS, SupportedShortcut};
use crate::updater::UpdateState;

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
    /// Screen-space rect of the Update Pill if it was painted this
    /// frame, used to anchor the popover.
    pub update_pill_rect: Option<egui::Rect>,
    /// True when the Update Pill was clicked this frame; the app
    /// toggles the popover in response.
    pub update_pill_clicked: bool,
    /// True when the Settings Tab "Check for updates now" button was
    /// activated this frame. The app forwards the request to the
    /// update service; throttling is the responsibility of the sidebar.
    pub update_check_now_clicked: bool,
    /// True when the Settings Tab "Rollback to previous version" button
    /// was activated this frame. The app performs the rollback through
    /// the `updater::rollback` module.
    pub rollback_clicked: bool,
    pub semantic_search_action: Option<SemanticSearchSettingsAction>,
}

/// Snapshot of the About / Updates section state. The sidebar reads but
/// does not mutate any of these; the app owns the data.
#[derive(Clone, Debug)]
pub(crate) struct AboutUpdatesView<'a> {
    /// Running app version, normally `env!("CARGO_PKG_VERSION")`.
    pub current_version: &'a str,
    /// Unix-epoch milliseconds of the last completed check. `None`
    /// renders as "never".
    pub last_checked_unix_millis: Option<u64>,
    /// True when a `.previous` / `.old` backup exists at the install
    /// root. Drives the visibility of the Rollback button.
    pub backup_present: bool,
    /// True when the "Check for updates now" button is currently
    /// allowed (i.e. last manual click was more than one minute ago).
    /// The button still paints but is disabled when this is false.
    pub check_now_enabled: bool,
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
    update_state: &UpdateState,
    about_updates: AboutUpdatesView<'_>,
) -> SidebarRenderResult {
    if !sidebar_open {
        return SidebarRenderResult {
            settings_changed: false,
            interactive_rect: None,
            board_action: None,
            update_pill_rect: None,
            update_pill_clicked: false,
            update_check_now_clicked: false,
            rollback_clicked: false,
            semantic_search_action: None,
        };
    }

    let mut settings_changed = false;
    let mut board_action = None;
    let mut update_pill_rect = None;
    let mut update_pill_clicked = false;
    let mut update_check_now_clicked = false;
    let mut rollback_clicked = false;
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
                ne: crate::theme::RADIUS_PANEL,
                sw: 0,
                se: crate::theme::RADIUS_PANEL,
            };
            ui.painter().rect(
                shape_rect,
                radius,
                crate::theme::SURFACE,
                crate::theme::stroke_subtle(),
                egui::StrokeKind::Inside,
            );

            // Reserve a bottom strip for the Update Pill so its presence
            // never collides with tab content height.
            let pill_strip_height = update_pill_strip_height(update_state);
            let content_bottom = shape_rect.bottom() - pill_strip_height;
            let inner_rect = egui::Rect::from_min_max(
                shape_rect.min,
                egui::pos2(shape_rect.right(), content_bottom),
            );
            let ui_builder = egui::UiBuilder::new().max_rect(inner_rect);
            ui.scope_builder(ui_builder, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                let mut paint_contents = |ui: &mut egui::Ui| {
                    paint_sidebar_tabs(ui, sidebar_tab);
                    ui.add_space(crate::theme::SPACE_2);
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
                            update_check_now_clicked |= options_outcome.check_now_clicked;
                            rollback_clicked |= options_outcome.rollback_clicked;
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

            if pill_strip_height > 0.0 {
                let pill_strip_rect = egui::Rect::from_min_max(
                    egui::pos2(shape_rect.left(), content_bottom),
                    egui::pos2(shape_rect.right(), shape_rect.bottom()),
                );
                let pill_outcome =
                    paint_update_pill(ui, pill_strip_rect, update_state, input_enabled);
                update_pill_rect = pill_outcome.rect;
                update_pill_clicked = pill_outcome.clicked;
            }
        });

    SidebarRenderResult {
        settings_changed,
        interactive_rect: Some(area_response.response.rect),
        board_action,
        update_pill_rect,
        update_pill_clicked,
        update_check_now_clicked,
        rollback_clicked,
        semantic_search_action,
    }
}

struct SidebarOptionsOutcome {
    settings_changed: bool,
    check_now_clicked: bool,
    rollback_clicked: bool,
    semantic_search_action: Option<SemanticSearchSettingsAction>,
}

struct UpdatePillOutcome {
    rect: Option<egui::Rect>,
    clicked: bool,
}

fn update_pill_strip_height(state: &UpdateState) -> f32 {
    if matches!(state, UpdateState::Hidden) {
        0.0
    } else {
        // Pill height + above/below padding.
        crate::theme::CONTROL_HEIGHT + crate::theme::SPACE_3 * 2.0
    }
}

fn paint_update_pill(
    ui: &mut egui::Ui,
    strip_rect: egui::Rect,
    state: &UpdateState,
    input_enabled: bool,
) -> UpdatePillOutcome {
    // Four observable shapes: clickable opener (Available), clickable
    // restarter (Ready), in-place progress bar (Downloading), quiet
    // failure indicator (Failed). Hidden returns early above.
    enum PillShape {
        Opens { label: String },
        Restart { label: String },
        Progress { label: String, percent: u8 },
        Quiet { label: String },
    }

    let shape = match state {
        UpdateState::Hidden => {
            return UpdatePillOutcome {
                rect: None,
                clicked: false,
            };
        }
        UpdateState::Available { version, .. } => PillShape::Opens {
            label: format!("Update {version}"),
        },
        UpdateState::Downloading { percent, .. } => PillShape::Progress {
            label: format!("Downloading {percent}%"),
            percent: (*percent).min(100),
        },
        UpdateState::Ready { .. } => PillShape::Restart {
            label: "Restart".to_owned(),
        },
        UpdateState::Failed => PillShape::Quiet {
            label: "Update check failed".to_owned(),
        },
    };

    let pad_x = crate::theme::SPACE_3;
    let pill_rect = egui::Rect::from_min_max(
        egui::pos2(
            strip_rect.left() + pad_x,
            strip_rect.top() + crate::theme::SPACE_3,
        ),
        egui::pos2(
            strip_rect.right() - pad_x,
            strip_rect.bottom() - crate::theme::SPACE_3,
        ),
    );
    let id = egui::Id::new("sidebar_update_pill");
    let is_clickable = matches!(shape, PillShape::Opens { .. } | PillShape::Restart { .. });
    let sense = if is_clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(pill_rect, id, sense);
    let interactive = input_enabled && is_clickable;
    let fill = crate::theme::SURFACE_ELEVATED;
    let stroke = if is_clickable {
        egui::Stroke::new(1.0, crate::theme::ACCENT)
    } else {
        crate::theme::stroke_subtle()
    };
    ui.painter().rect(
        pill_rect,
        crate::theme::RADIUS_COMPACT,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );

    // Progress bar fill — painted before the label so the text sits on top.
    if let PillShape::Progress { percent, .. } = &shape {
        let fraction = (*percent as f32 / 100.0).clamp(0.0, 1.0);
        if fraction > 0.0 {
            let bar_width = (pill_rect.width() * fraction).max(1.0);
            let bar_rect = egui::Rect::from_min_max(
                pill_rect.min,
                egui::pos2(pill_rect.min.x + bar_width, pill_rect.max.y),
            );
            ui.painter().rect_filled(
                bar_rect,
                crate::theme::RADIUS_COMPACT,
                crate::theme::ACCENT.gamma_multiply(0.25),
            );
        }
    }

    let (label, text_color) = match &shape {
        PillShape::Opens { label } | PillShape::Restart { label } => {
            (label.clone(), crate::theme::TEXT)
        }
        PillShape::Progress { label, .. } => (label.clone(), crate::theme::TEXT),
        PillShape::Quiet { label } => (label.clone(), crate::theme::TEXT_MUTED),
    };
    let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let galley = ui.painter().layout_no_wrap(label, font, text_color);
    let label_pos = egui::pos2(
        pill_rect.center().x - galley.size().x * 0.5,
        pill_rect.center().y - galley.size().y * 0.5 - 1.0,
    );
    ui.painter().galley(label_pos, galley, text_color);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Update")
    });
    UpdatePillOutcome {
        rect: Some(pill_rect),
        clicked: interactive && response.clicked(),
    }
}

fn paint_sidebar_tabs(ui: &mut egui::Ui, sidebar_tab: &mut SidebarTab) {
    let bar_height = 32.0;
    let (_id, rect) = ui.allocate_space(egui::vec2(ui.available_width(), bar_height + 1.0));
    let pad_x = crate::theme::SPACE_3;
    let tab_rect = egui::Rect::from_min_max(egui::pos2(rect.left() + pad_x, rect.top()), rect.max);
    let tabs: [(&str, SidebarTab); 2] = [
        ("Boards", SidebarTab::Boards),
        ("Settings", SidebarTab::Options),
    ];
    let tab_width = (tab_rect.width() - pad_x) / tabs.len() as f32;
    for (index, (label, tab_value)) in tabs.iter().enumerate() {
        let cell_min = egui::pos2(tab_rect.left() + tab_width * index as f32, tab_rect.top());
        let cell = egui::Rect::from_min_size(cell_min, egui::vec2(tab_width, bar_height));
        let selected = *sidebar_tab == *tab_value;
        let response = ui.interact(
            cell,
            egui::Id::new(("sidebar_tab", label)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), *label)
        });
        if response.clicked() {
            *sidebar_tab = *tab_value;
        }
        let label_color = if selected {
            crate::theme::TEXT
        } else if response.hovered() {
            crate::theme::TEXT_MUTED
        } else {
            crate::theme::TEXT_SUBTLE
        };
        let font = ui.style().text_styles[&egui::TextStyle::Body].clone();
        let galley = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), font, label_color);
        let label_pos = egui::pos2(
            cell.center().x - galley.size().x * 0.5,
            cell.center().y - galley.size().y * 0.5 - 2.0,
        );
        ui.painter().galley(label_pos, galley, label_color);
        if selected {
            let underline_y = cell.bottom();
            let underline = egui::Rect::from_min_max(
                egui::pos2(cell.center().x - 18.0, underline_y - 1.5),
                egui::pos2(cell.center().x + 18.0, underline_y),
            );
            ui.painter().rect_filled(underline, 0, crate::theme::ACCENT);
        }
    }
}

fn paint_sidebar_options(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    about_updates: &AboutUpdatesView<'_>,
) -> SidebarOptionsOutcome {
    let mut changed = false;
    let mut check_now_clicked = false;
    let mut rollback_clicked = false;
    let mut semantic_search_action = None;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        egui::ScrollArea::vertical()
            .id_salt("sidebar_options_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.add_space(crate::theme::SPACE_3);
                paint_sidebar_subcard(ui, |ui| {
                    let about_outcome = paint_about_updates_section(ui, about_updates);
                    check_now_clicked |= about_outcome.check_now_clicked;
                    rollback_clicked |= about_outcome.rollback_clicked;
                });
                ui.add_space(crate::theme::SPACE_4);
                paint_sidebar_subcard(ui, |ui| {
                    semantic_search_action =
                        paint_semantic_search_settings_section(ui, &about_updates.semantic_search);
                });
                ui.add_space(crate::theme::SPACE_4);
                paint_sidebar_subcard(ui, |ui| {
                    changed |= paint_settings_section(ui, settings, canvas);
                });
                ui.add_space(crate::theme::SPACE_6);
                paint_sidebar_section_header(ui, "Shortcuts", None);
                paint_supported_shortcuts_section(ui);
                ui.add_space(crate::theme::SPACE_4);
            });
    });
    SidebarOptionsOutcome {
        settings_changed: changed,
        check_now_clicked,
        rollback_clicked,
        semantic_search_action,
    }
}

struct AboutUpdatesOutcome {
    check_now_clicked: bool,
    rollback_clicked: bool,
}

fn paint_about_updates_section(
    ui: &mut egui::Ui,
    about: &AboutUpdatesView<'_>,
) -> AboutUpdatesOutcome {
    let mut check_now_clicked = false;
    let mut rollback_clicked = false;

    ui.add_space(crate::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
        ui.label(
            egui::RichText::new("About / Updates")
                .text_style(egui::TextStyle::Body)
                .color(crate::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(format!("Version {}", about.current_version))
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_SUBTLE),
        );
        ui.label(
            egui::RichText::new(format!(
                "Last checked: {}",
                format_last_checked(about.last_checked_unix_millis),
            ))
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_SUBTLE),
        );
        ui.add_space(crate::theme::SPACE_2);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
            let check_button = egui::Button::new(
                egui::RichText::new("Check for updates now")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT),
            );
            if ui
                .add_enabled(about.check_now_enabled, check_button)
                .clicked()
            {
                check_now_clicked = true;
            }
            if about.backup_present {
                let rollback_button = egui::Button::new(
                    egui::RichText::new("Rollback to previous version")
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT),
                );
                if ui.add(rollback_button).clicked() {
                    rollback_clicked = true;
                }
            }
        });
    });
    ui.add_space(crate::theme::SPACE_2 + 2.0);

    AboutUpdatesOutcome {
        check_now_clicked,
        rollback_clicked,
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

fn paint_semantic_search_settings_section(
    ui: &mut egui::Ui,
    semantic_search: &SemanticSearchSettingsView,
) -> Option<SemanticSearchSettingsAction> {
    let mut action = None;
    ui.add_space(crate::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
        ui.label(
            egui::RichText::new("Semantic Search")
                .text_style(egui::TextStyle::Body)
                .color(crate::theme::TEXT),
        );
        let (status, detail) = semantic_search_status_copy(&semantic_search.status);
        ui.label(
            egui::RichText::new(status)
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(detail)
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE),
            )
            .wrap(),
        );
        ui.add_space(crate::theme::SPACE_2);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
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
    ui.add_space(crate::theme::SPACE_2 + 2.0);
    action
}

fn semantic_search_status_copy(status: &SemanticSearchSettingsStatus) -> (String, &'static str) {
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
            "Try installing Semantic Search again after checking your connection.",
        ),
    }
}

fn quiet_settings_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT),
    )
    .fill(crate::theme::SURFACE_ELEVATED)
    .stroke(crate::theme::stroke_subtle())
    .corner_radius(crate::theme::RADIUS_INLINE)
}

fn paint_sidebar_boards(
    ui: &mut egui::Ui,
    boards: &[crate::data::BoardSummary],
    active_board_id: i64,
    forced_hover_index: Option<usize>,
) -> Option<SidebarBoardAction> {
    let mut board_action = None;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("sidebar_boards_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                ui.add_space(crate::theme::SPACE_3);
                let indent = crate::theme::SPACE_2;
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        egui::pos2(ui.max_rect().left() + indent, ui.max_rect().top()),
                        egui::pos2(ui.max_rect().right() - indent, ui.max_rect().bottom()),
                    )),
                    |ui| {
                        for (index, summary) in boards.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(crate::theme::SPACE_1);
                            }
                            let shortcut = board_shortcut_hint(index);
                            match paint_board_row(
                                ui,
                                summary.board.id,
                                &summary.board.name,
                                &image_count_label(summary.image_count),
                                summary.board.id == active_board_id,
                                shortcut.as_deref(),
                                forced_hover_index == Some(index),
                            ) {
                                Some(BoardRowAction::Switch) => {
                                    board_action =
                                        Some(SidebarBoardAction::SwitchBoard(summary.board.id));
                                }
                                Some(BoardRowAction::Rename(name)) => {
                                    board_action = Some(SidebarBoardAction::RenameBoard(
                                        summary.board.id,
                                        name,
                                    ));
                                }
                                Some(BoardRowAction::Delete) => {
                                    board_action = Some(SidebarBoardAction::RequestDeleteBoard(
                                        summary.board.id,
                                    ));
                                }
                                None => {}
                            }
                        }
                        ui.add_space(crate::theme::SPACE_3);
                        if paint_create_board_button(ui) {
                            board_action = Some(SidebarBoardAction::CreateBoard);
                        }
                    },
                );
            });
    });
    board_action
}

enum BoardRowAction {
    Switch,
    Rename(String),
    Delete,
}

fn paint_board_row(
    ui: &mut egui::Ui,
    board_id: i64,
    name: &str,
    detail: &str,
    active: bool,
    shortcut: Option<&str>,
    forced_hover: bool,
) -> Option<BoardRowAction> {
    let height = 48.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let name_edit_id = ui.make_persistent_id(("board-name-edit", board_id));
    let renaming = ui.memory(|memory| memory.has_focus(name_edit_id));
    let pointer_in_row = ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|pointer_pos| rect.contains(pointer_pos))
    });
    let hot = pointer_in_row || forced_hover || renaming;
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

    let swatch_size = 24.0;
    let swatch_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + crate::theme::SPACE_3,
            rect.center().y - swatch_size * 0.5,
        ),
        egui::vec2(swatch_size, swatch_size),
    );
    ui.painter().rect_filled(
        swatch_rect,
        egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
        egui::Color32::from_white_alpha(if active { 38 } else { 18 }),
    );

    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let detail_galley =
        ui.painter()
            .layout_no_wrap(detail.to_owned(), detail_font, crate::theme::TEXT_MUTED);

    let text_x = swatch_rect.right() + crate::theme::SPACE_2;
    let action_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 32.0, rect.center().y),
        egui::vec2(56.0, 24.0),
    );
    let text_right = action_rect.left() - crate::theme::SPACE_2;
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.top() + crate::theme::SPACE_2),
        egui::pos2(text_right.max(text_x + 24.0), rect.top() + 27.0),
    );
    let title_hit_id = ui.make_persistent_id(("board-name-hit", board_id));
    let title_response = ui.interact(name_rect, title_hit_id, egui::Sense::click());
    title_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Board {name}"),
        )
    });
    let title_double_clicked = title_response.double_clicked();
    let mut submitted_name = None;
    if title_double_clicked {
        ui.memory_mut(|memory| memory.request_focus(name_edit_id));
        ui.data_mut(|data| data.insert_persisted(name_edit_id, name.to_owned()));
    }
    if renaming {
        let mut name_edit = ui
            .data_mut(|data| data.get_persisted::<String>(name_edit_id))
            .unwrap_or_else(|| name.to_owned());
        let name_response = ui.place(
            name_rect,
            egui::TextEdit::singleline(&mut name_edit)
                .id(name_edit_id)
                .font(egui::TextStyle::Body)
                .text_color(crate::theme::TEXT)
                .frame(egui::Frame::NONE)
                .desired_width(name_rect.width()),
        );
        ui.data_mut(|data| data.insert_persisted(name_edit_id, name_edit.clone()));
        let rename_submitted = name_response.lost_focus()
            && name_edit != name
            && ui.input(|input| input.key_pressed(egui::Key::Enter) || input.pointer.any_click());
        if rename_submitted {
            submitted_name = Some(name_edit);
        }
    } else {
        let title_galley = ui.painter().layout_no_wrap(
            name.to_owned(),
            egui::TextStyle::Body.resolve(ui.style()),
            crate::theme::TEXT,
        );
        ui.painter()
            .galley(name_rect.left_top(), title_galley, crate::theme::TEXT);
    }

    let detail_pos = egui::pos2(text_x, rect.top() + 28.0);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::theme::TEXT_MUTED);

    let delete_response = ui.interact(
        action_rect,
        ui.make_persistent_id(("board-delete-hit", board_id)),
        egui::Sense::click(),
    );
    delete_response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Delete board {name}"),
        )
    });
    let delete_hot = hot || delete_response.hovered();
    if delete_hot {
        ui.painter().text(
            action_rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            "Delete",
            egui::TextStyle::Small.resolve(ui.style()),
            crate::theme::DESTRUCTIVE,
        );
    } else if let Some(shortcut) = shortcut {
        let font = ui.style().text_styles[&egui::TextStyle::Small].clone();
        let galley = ui.painter().layout_no_wrap(
            display_shortcut_label(shortcut).to_owned(),
            font,
            crate::theme::TEXT_SUBTLE,
        );
        let pos = egui::pos2(
            action_rect.right() - galley.size().x,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(pos, galley, crate::theme::TEXT_SUBTLE);
    }
    if delete_hot && delete_response.clicked() {
        Some(BoardRowAction::Delete)
    } else if let Some(name_edit) = submitted_name {
        Some(BoardRowAction::Rename(name_edit))
    } else if !title_double_clicked && (response.clicked() || title_response.clicked()) {
        Some(BoardRowAction::Switch)
    } else {
        None
    }
}

fn paint_create_board_button(ui: &mut egui::Ui) -> bool {
    let height = 36.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "New board")
    });
    let dash_stroke = egui::Stroke::new(1.0, crate::theme::BORDER);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_COMPACT),
        if response.hovered() {
            crate::theme::SURFACE_ELEVATED
        } else {
            egui::Color32::from_white_alpha(4)
        },
        dash_stroke,
        egui::StrokeKind::Inside,
    );

    let plus_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let plus_galley = ui.painter().layout_no_wrap(
        "+  New board".to_owned(),
        plus_font,
        crate::theme::TEXT_SUBTLE,
    );
    let pos = egui::pos2(
        rect.center().x - plus_galley.size().x * 0.5,
        rect.center().y - plus_galley.size().y * 0.5 - 1.0,
    );
    ui.painter()
        .galley(pos, plus_galley, crate::theme::TEXT_SUBTLE);
    response.clicked()
}

fn image_count_label(count: usize) -> String {
    match count {
        1 => "1 image".to_owned(),
        _ => format!("{count} images"),
    }
}

fn board_shortcut_hint(index: usize) -> Option<String> {
    let modifier = platform_command_modifier_name();
    match index {
        0..=8 => Some(format!("{modifier}+{}", index + 1)),
        9 => Some(format!("{modifier}+0")),
        _ => None,
    }
}

fn paint_sidebar_section_header(ui: &mut egui::Ui, title: &str, detail: Option<&str>) {
    let pad = crate::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.cursor().top() + 22.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
            ui.label(
                egui::RichText::new(title)
                    .text_style(crate::theme::display_text_style())
                    .color(crate::theme::TEXT),
            );
            if let Some(detail) = detail {
                ui.with_layout(crate::ui_layout::row_end_center_y_layout(), |ui| {
                    ui.label(
                        egui::RichText::new(detail)
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE),
                    );
                });
            }
        });
    });
    ui.add_space(22.0 + crate::theme::SPACE_2);
}

fn paint_sidebar_subcard(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let pad = crate::theme::SPACE_2;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from_white_alpha(5))
            .stroke(crate::theme::stroke_subtle())
            .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
            .inner_margin(egui::Margin {
                left: crate::theme::SPACE_3 as i8,
                right: crate::theme::SPACE_3 as i8,
                top: crate::theme::SPACE_1 as i8,
                bottom: crate::theme::SPACE_1 as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                add_contents(ui);
            });
    });
}

fn paint_settings_section(
    ui: &mut egui::Ui,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;

    changed |= paint_setting_row(
        ui,
        "Pointer mode",
        pointer_mode_detail(settings.pointer_mode),
        |ui| paint_pointer_mode_control(ui, &mut settings.pointer_mode),
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
            ui.add(
                egui::DragValue::new(&mut settings.rotation_snap_degrees)
                    .speed(1.0)
                    .range(0.0..=180.0)
                    .fixed_decimals(0)
                    .suffix(" deg"),
            )
            .changed()
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
        "Import image area",
        "Target area for auto-scaled imports.",
        |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.import_target_canvas_area)
                    .speed(1024.0)
                    .range(10_000.0..=1_000_000.0)
                    .fixed_decimals(0)
                    .suffix(" units"),
            )
            .changed()
        },
    );
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Canvas cache max side",
        "Largest cached canvas image side.",
        |ui| {
            ui.add(
                egui::DragValue::new(&mut settings.canvas_image_max_side)
                    .speed(64.0)
                    .range(256..=8192)
                    .fixed_decimals(0)
                    .suffix(" px"),
            )
            .changed()
        },
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
        ui.add(
            egui::DragValue::new(&mut settings.import_gap)
                .speed(0.5)
                .range(0.0..=128.0)
                .fixed_decimals(0)
                .suffix(" units"),
        )
        .changed()
    });

    changed
}

fn paint_setting_row(
    ui: &mut egui::Ui,
    title: &str,
    detail: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> bool,
) -> bool {
    let mut changed = false;
    ui.add_space(crate::theme::SPACE_2 + 2.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 1.0);
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
        changed |= add_control(ui);
    });
    ui.add_space(crate::theme::SPACE_2 + 2.0);
    changed
}

fn paint_setting_divider(ui: &mut egui::Ui) {
    let available = ui.available_width();
    let pad = crate::theme::SPACE_3;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, 1.0), egui::Sense::hover());
    let from = egui::pos2(rect.left() - pad, rect.center().y);
    let to = egui::pos2(rect.right() + pad, rect.center().y);
    ui.painter()
        .line_segment([from, to], crate::theme::stroke_subtle());
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
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
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
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
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
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let pad = egui::vec2(10.0, 5.0);
    let size = egui::vec2((galley.size().x + pad.x * 2.0).max(32.0), 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let fill = if selected {
        egui::Color32::from_white_alpha(12)
    } else if response.hovered() {
        egui::Color32::from_white_alpha(7)
    } else {
        egui::Color32::TRANSPARENT
    };
    if selected || response.hovered() {
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(crate::theme::RADIUS_INLINE),
            fill,
            if selected {
                crate::theme::stroke_subtle()
            } else {
                egui::Stroke::NONE
            },
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().galley(
        crate::ui_layout::centered_pos(rect, galley.size()),
        galley,
        text_color,
    );
    response
}

fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    let pad = crate::theme::SPACE_3;
    let indent_rect = egui::Rect::from_min_max(
        egui::pos2(ui.max_rect().left() + pad, ui.cursor().top()),
        egui::pos2(ui.max_rect().right() - pad, ui.max_rect().bottom()),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(indent_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        ui.label(
            egui::RichText::new("Implemented")
                .text_style(egui::TextStyle::Body)
                .strong()
                .color(crate::theme::TEXT),
        );
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("Bindings grouped by task.")
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_SUBTLE),
        );
        ui.add_space(crate::theme::SPACE_3);

        let (core_shortcuts, layout_shortcuts) = SUPPORTED_SHORTCUTS.split_at(9);
        paint_shortcut_group(ui, "Core actions", core_shortcuts);
        ui.add_space(crate::theme::SPACE_3);
        paint_shortcut_group(ui, "Layout + history", layout_shortcuts);
    });
}

fn paint_shortcut_group(ui: &mut egui::Ui, title: &str, shortcuts: &[SupportedShortcut]) {
    ui.label(
        egui::RichText::new(title)
            .text_style(egui::TextStyle::Small)
            .strong()
            .color(crate::theme::TEXT_MUTED),
    );
    ui.add_space(crate::theme::SPACE_1);
    for (index, shortcut) in shortcuts.iter().enumerate() {
        if index > 0 {
            paint_shortcut_divider(ui);
        }
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
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_2;
        ui.with_layout(crate::ui_layout::row_end_center_y_layout(), |ui| {
            paint_shortcut_pill(ui, keys);
            ui.with_layout(crate::ui_layout::row_center_y_layout(), |ui| {
                ui.label(
                    egui::RichText::new(shortcut.action)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT),
                );
            });
        });
    });
    ui.add_space(3.0);
}

fn paint_shortcut_pill(ui: &mut egui::Ui, keys: &str) {
    let keys = display_shortcut_label(keys);
    let mut font = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    if shortcut_label_uses_small_symbol_glyphs(&keys) {
        font.size += 3.0;
    }
    let galley = ui.painter().layout_no_wrap(keys, font, crate::theme::TEXT);
    let pad = egui::vec2(6.0, 1.0);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::theme::RADIUS_MICRO),
        egui::Color32::from_white_alpha(15),
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.min + pad, galley, crate::theme::TEXT);
}

fn paint_shortcut_divider(ui: &mut egui::Ui) {
    let available = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.center().y),
            egui::pos2(rect.right(), rect.center().y),
        ],
        crate::theme::stroke_subtle(),
    );
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

pub(crate) fn show_sidebar_hint(
    ctx: &egui::Context,
    screen_rect: egui::Rect,
    sidebar_open: bool,
    active_board_name: &str,
) -> bool {
    if sidebar_open {
        return false;
    }

    let mut clicked = false;
    egui::Area::new(egui::Id::new("sidebar_hint"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.left() + crate::theme::SPACE_4,
            screen_rect.bottom() - crate::theme::SPACE_4,
        ))
        .anchor(
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(crate::theme::SPACE_4, -crate::theme::SPACE_4),
        )
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(crate::theme::SURFACE_ELEVATED)
                .stroke(crate::theme::stroke_subtle())
                .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_COMPACT))
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 6,
                    bottom: 6,
                })
                .show(ui, |ui| {
                    let label = format!(
                        "{}  {}",
                        truncate_board_name(active_board_name, 28),
                        display_shortcut_label(sidebar_shortcut_label())
                    );
                    let galley = ui.painter().layout_no_wrap(
                        label,
                        egui::TextStyle::Small.resolve(ui.style()),
                        crate::theme::TEXT_MUTED,
                    );
                    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
                    ui.painter()
                        .galley(rect.min, galley, crate::theme::TEXT_MUTED);
                    let hovered = ui.ctx().input(|input| {
                        input
                            .pointer
                            .latest_pos()
                            .is_some_and(|pos| rect.contains(pos))
                    });
                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    clicked = hovered && ui.input(|input| input.pointer.primary_clicked());
                });
        });
    clicked
}

fn truncate_board_name(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_owned();
    }
    let mut truncated = name
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn sidebar_shortcut_label() -> &'static str {
    platform_shortcut_label("⌘B", "Ctrl+B")
}

fn platform_command_modifier_name() -> &'static str {
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

fn display_shortcut_label(keys: &str) -> String {
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

fn shortcut_label_uses_small_symbol_glyphs(keys: &str) -> bool {
    keys.chars().any(|ch| matches!(ch, '⇧' | '↵'))
}

#[cfg(test)]
mod tests {
    use super::{
        SemanticSearchSettingsStatus, display_shortcut_label, paint_board_row,
        semantic_search_status_copy, shortcut_label_uses_small_symbol_glyphs,
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
        assert_eq!(
            semantic_search_status_copy(&SemanticSearchSettingsStatus::Installed).0,
            "Installed"
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
