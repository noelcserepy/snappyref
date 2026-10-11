//! The sidebar: one floating card off the window's left edge holding the
//! Boards list and Settings, with the tab between them pinned to its
//! bottom so switching never moves the card.

use eframe::egui;

mod about;
mod boards;
mod controls;
mod icons;
mod keys;
mod settings;

use crate::ui::{card, motion, theme};

pub(crate) use about::{AboutView, AppUpdateStatus};
pub(crate) use controls::{show_sidebar_edge, show_status_message};

/// The card's gap to the window edge.
const INSET: f32 = 8.0;
/// The Group card's width, so both cards share one inner column.
const CARD_WIDTH: f32 = 264.0;
const INNER_WIDTH: f32 = CARD_WIDTH - 2.0 * card::PADDING;
/// Clear of the macOS title bar.
const TOP_MARGIN: f32 = 40.0;
/// The window space the open sidebar covers.
pub(crate) const SIDEBAR_WIDTH: f32 = INSET + CARD_WIDTH;
/// The Boards/Settings tab: a bed around control-height tabs.
const TAB_HEIGHT: f32 = theme::CONTROL_HEIGHT + 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SidebarTab {
    #[default]
    Boards,
    Settings,
}

pub(crate) struct SidebarRenderResult {
    pub settings_changed: bool,
    pub interactive_rect: Option<egui::Rect>,
    pub board_action: Option<SidebarBoardAction>,
    /// True when the About update action was activated this frame.
    pub update_action_clicked: bool,
    pub semantic_search_action: Option<SemanticSearchSettingsAction>,
    pub copy_mcp_config_clicked: bool,
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
    Create,
    Switch(i64),
    Rename(i64, String),
    RequestDelete(i64),
}

/// The open sidebar's card.
pub(crate) fn sidebar_rect(screen_rect: egui::Rect) -> egui::Rect {
    let top = screen_rect.top() + TOP_MARGIN;
    let bottom = (screen_rect.bottom() - INSET).max(top);
    egui::Rect::from_min_max(
        egui::pos2(screen_rect.left() + INSET, top),
        egui::pos2(screen_rect.left() + SIDEBAR_WIDTH, bottom),
    )
}

pub(crate) fn block_sidebar_input(ctx: &egui::Context, sidebar_rect: egui::Rect) {
    if sidebar_rect.height() <= 0.0 || sidebar_rect.width() <= 0.0 {
        return;
    }

    // Modal dialogs must block sidebar interaction without painting anything.
    // A visible frame here reads as a transient sidebar outline.
    egui::Area::new(egui::Id::new("sidebar_blocker"))
        .order(egui::Order::Tooltip)
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
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !sidebar_rect.contains(pos))
    });

    if clicked_outside {
        *sidebar_open_state = false;
    }
}

#[allow(clippy::too_many_arguments)]
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
    about: &AboutView,
    semantic_search: &SemanticSearchSettingsStatus,
) -> SidebarRenderResult {
    let entrance = motion::overlay_entrance(ctx, egui::Id::new("sidebar_entrance"), sidebar_open);
    let mut result = SidebarRenderResult {
        settings_changed: false,
        interactive_rect: None,
        board_action: None,
        update_action_clicked: false,
        semantic_search_action: None,
        copy_mcp_config_clicked: false,
    };
    if !sidebar_open {
        return result;
    }

    let rect =
        sidebar_rect(screen_rect).translate(egui::vec2(-motion::settle_offset(entrance), 0.0));
    let inner_size = rect.size() - egui::Vec2::splat(2.0 * card::PADDING);
    let area = egui::Area::new(egui::Id::new("sidebar"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_opacity(entrance.clamp(0.0, 1.0));
            card::frame().show(ui, |ui| {
                let inner = egui::Rect::from_min_size(ui.cursor().min, inner_size);
                ui.set_min_size(inner_size);
                ui.spacing_mut().item_spacing = card::ITEM_SPACING;
                let tab_rect = egui::Rect::from_min_max(
                    egui::pos2(inner.left(), inner.bottom() - TAB_HEIGHT),
                    inner.max,
                );
                // The body spans the padding too, so its scroll bar sits
                // there. A child Ui keeps it from widening the frame.
                let body = egui::Rect::from_min_max(
                    egui::pos2(rect.left(), inner.top()),
                    egui::pos2(rect.right(), tab_rect.top() - 14.0),
                );
                let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
                match sidebar_tab {
                    SidebarTab::Boards => {
                        result.board_action = boards::show(&mut body_ui, boards, active_board_id);
                    }
                    SidebarTab::Settings => {
                        settings::show(
                            &mut body_ui,
                            settings,
                            canvas,
                            about,
                            semantic_search,
                            &mut result,
                        );
                    }
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(tab_rect), |ui| {
                    let tabs = [SidebarTab::Boards, SidebarTab::Settings];
                    let open = tabs.iter().position(|tab| tab == sidebar_tab).unwrap_or(0);
                    let names = ["Boards", "Settings"];
                    if let Some(i) = card::tabs(
                        ui,
                        egui::vec2(INNER_WIDTH, TAB_HEIGHT),
                        &names,
                        open,
                        |p, i, r, ink| {
                            p.text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                names[i],
                                egui::FontId::proportional(theme::TYPE_CONTROL),
                                ink,
                            );
                        },
                    ) {
                        *sidebar_tab = tabs[i];
                    }
                });
            });
        });
    result.interactive_rect = Some(area.response.rect);

    if !input_enabled {
        result.settings_changed = false;
        result.board_action = None;
        result.update_action_clicked = false;
        result.semantic_search_action = None;
        result.copy_mcp_config_clicked = false;
    }
    result
}

/// A vertical scroll filling the body, with contents inset to the card's
/// padding.
fn scroll(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(card::PADDING as i8, 0))
                .show(ui, |ui| {
                    ui.set_width(INNER_WIDTH);
                    ui.spacing_mut().item_spacing = card::ITEM_SPACING;
                    contents(ui);
                });
        });
}

/// The body's inner column for content that stays put above a scroll.
fn column<R>(ui: &mut egui::Ui, height: f32, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(card::PADDING, 0.0))),
        contents,
    )
    .inner
}
