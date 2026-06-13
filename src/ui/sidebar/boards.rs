use eframe::egui;

use super::{SidebarBoardAction, shortcuts};

/// Paints the Boards Tab navigation list.
///
/// Board rows are a product-specific navigation pattern: text-first, compact,
/// active rail + quiet fill, bare shortcut metadata, and a destructive delete
/// affordance that stays quiet until the row is hot. Do not reuse this row
/// painter for generic lists; it assumes board switching, renaming, deletion,
/// and stable row geometry are all part of the same interaction.
pub(super) fn paint_sidebar_boards(
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
                ui.add_space(crate::ui::theme::SPACE_3);
                let indent = crate::ui::theme::SPACE_2;
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        egui::pos2(ui.max_rect().left() + indent, ui.max_rect().top()),
                        egui::pos2(ui.max_rect().right() - indent, ui.max_rect().bottom()),
                    )),
                    |ui| {
                        let boards_cascade = egui::Id::new("sidebar_boards_cascade");
                        let list_top = ui.cursor().top();
                        let mut active_row_rect = None;
                        for (index, summary) in boards.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(crate::ui::theme::SPACE_1);
                            }
                            let shortcut = board_shortcut_hint(index);
                            let active = summary.board.id == active_board_id;
                            let reveal = crate::ui::motion::cascade_reveal_on_appear(
                                ui.ctx(),
                                boards_cascade,
                                index,
                            );
                            let row_top = ui.cursor().top();
                            let action = ui
                                .scope(|ui| {
                                    ui.multiply_opacity(reveal.opacity);
                                    paint_board_row(
                                        ui,
                                        summary.board.id,
                                        &summary.board.name,
                                        &placement_count_label(summary.placement_count),
                                        active,
                                        shortcut.as_deref(),
                                        forced_hover_index == Some(index),
                                        reveal.rise,
                                    )
                                })
                                .inner;
                            if active {
                                active_row_rect = Some(egui::Rect::from_min_size(
                                    egui::pos2(ui.max_rect().left(), row_top),
                                    egui::vec2(ui.max_rect().width(), BOARD_ROW_HEIGHT),
                                ));
                            }
                            match action {
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
                        // Active-board indicator: one accent rail that slides
                        // between rows instead of jumping. Animated in
                        // list-local coordinates so scrolling does not lag.
                        if let Some(active_rect) = active_row_rect {
                            let rail_top_relative = crate::ui::motion::slide_indicator(
                                ui.ctx(),
                                egui::Id::new("sidebar_board_rail_top"),
                                active_rect.top() - list_top,
                            );
                            let rail_top = list_top + rail_top_relative;
                            let rail_rect = egui::Rect::from_min_max(
                                egui::pos2(
                                    active_rect.left(),
                                    rail_top + crate::ui::theme::SPACE_2,
                                ),
                                egui::pos2(
                                    active_rect.left() + 2.0,
                                    rail_top + BOARD_ROW_HEIGHT - crate::ui::theme::SPACE_2,
                                ),
                            );
                            ui.painter().rect_filled(
                                rail_rect,
                                egui::CornerRadius::same(crate::ui::theme::RADIUS_MICRO),
                                crate::ui::theme::ACCENT,
                            );
                        }
                        ui.add_space(crate::ui::theme::SPACE_3);
                        if paint_create_board_button(ui) {
                            board_action = Some(SidebarBoardAction::CreateBoard);
                        }
                    },
                );
            });
    });
    board_action
}

pub(super) enum BoardRowAction {
    Switch,
    Rename(String),
    Delete,
}

pub(super) const BOARD_ROW_HEIGHT: f32 = 48.0;

/// Board navigation rows allocate a fixed rect and paint text/action states
/// manually so hover, press, shortcut replacement, and rename focus cannot
/// shift list geometry.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_board_row(
    ui: &mut egui::Ui,
    board_id: i64,
    name: &str,
    detail: &str,
    active: bool,
    shortcut: Option<&str>,
    forced_hover: bool,
    rise: f32,
) -> Option<BoardRowAction> {
    let height = BOARD_ROW_HEIGHT;
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
    let hover = crate::ui::motion::hover_amount(
        ui.ctx(),
        ui.make_persistent_id(("board-row-hover", board_id)),
        hot && !active,
    );
    let press = crate::ui::motion::press_amount(
        ui.ctx(),
        ui.make_persistent_id(("board-row-press", board_id)),
        response.is_pointer_button_down_on(),
    );
    let fill = if active {
        crate::ui::theme::QUIET_INLINE_ACTIVE_FILL
    } else if hover > 0.0 {
        crate::ui::theme::QUIET_INLINE_HOVER_FILL.gamma_multiply(hover)
    } else {
        egui::Color32::TRANSPARENT
    };
    let fill = crate::ui::motion::mix(fill, crate::ui::theme::SURFACE_PRESSED, press * 0.65);
    let paint_rect = crate::ui::motion::pressed_rect(rect, press);
    ui.painter().rect(
        paint_rect,
        egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
        fill,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );

    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let detail_galley = ui.painter().layout_no_wrap(
        detail.to_owned(),
        detail_font,
        crate::ui::theme::TEXT_SUBTLE,
    );

    let text_x = rect.left() + crate::ui::theme::SPACE_3;
    let action_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 32.0, rect.center().y),
        egui::vec2(56.0, 24.0),
    );
    let text_right = action_rect.left() - crate::ui::theme::SPACE_2;
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.top() + crate::ui::theme::SPACE_2),
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
                .text_color(crate::ui::theme::TEXT)
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
            crate::ui::theme::TEXT,
        );
        ui.painter().galley(
            name_rect.left_top() + egui::vec2(0.0, rise + press * 0.5),
            title_galley,
            crate::ui::theme::TEXT,
        );
    }

    let detail_pos = egui::pos2(text_x, rect.top() + 28.0 + rise + press * 0.5);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::ui::theme::TEXT_SUBTLE);

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
            crate::ui::theme::DESTRUCTIVE,
        );
    } else if let Some(shortcut) = shortcut {
        let font = ui.style().text_styles[&egui::TextStyle::Small].clone();
        let galley = ui.painter().layout_no_wrap(
            shortcuts::display_shortcut_label(shortcut).to_owned(),
            font,
            crate::ui::theme::TEXT_SUBTLE,
        );
        let pos = egui::pos2(
            action_rect.right() - galley.size().x,
            rect.center().y - galley.size().y * 0.5 + rise + press * 0.5,
        );
        ui.painter()
            .galley(pos, galley, crate::ui::theme::TEXT_SUBTLE);
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
    let hover = crate::ui::motion::hover_amount(
        ui.ctx(),
        egui::Id::new("create-board-hover"),
        response.hovered(),
    );
    let press = crate::ui::motion::press_amount(
        ui.ctx(),
        egui::Id::new("create-board-press"),
        response.is_pointer_button_down_on(),
    );
    let button_rect = crate::ui::motion::pressed_rect(rect, press);
    if hover > 0.0 || press > 0.0 {
        ui.painter().rect(
            button_rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
            crate::ui::motion::mix(
                crate::ui::theme::QUIET_INLINE_HOVER_FILL.gamma_multiply(hover),
                crate::ui::theme::SURFACE_PRESSED,
                press * 0.65,
            ),
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
    }

    let label_color =
        crate::ui::motion::mix(crate::ui::theme::TEXT_SUBTLE, crate::ui::theme::TEXT, hover);
    let plus_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let plus_galley =
        ui.painter()
            .layout_no_wrap("+  New board".to_owned(), plus_font, label_color);
    let pos = egui::pos2(
        rect.left() + crate::ui::theme::SPACE_3,
        rect.center().y - plus_galley.size().y * 0.5 - 1.0 + press * 0.5,
    );
    ui.painter().galley(pos, plus_galley, label_color);
    response.clicked()
}

fn placement_count_label(count: usize) -> String {
    match count {
        1 => "1 image".to_owned(),
        _ => format!("{count} images"),
    }
}

fn board_shortcut_hint(index: usize) -> Option<String> {
    let modifier = shortcuts::platform_command_modifier_name();
    match index {
        0..=8 => Some(format!("{modifier}+{}", index + 1)),
        9 => Some(format!("{modifier}+0")),
        _ => None,
    }
}
