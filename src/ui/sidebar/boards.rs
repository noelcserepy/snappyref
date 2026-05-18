use eframe::egui;

use super::{SidebarBoardAction, shortcuts};

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
                        for (index, summary) in boards.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(crate::ui::theme::SPACE_1);
                            }
                            let shortcut = board_shortcut_hint(index);
                            match paint_board_row(
                                ui,
                                summary.board.id,
                                &summary.board.name,
                                &placement_count_label(summary.placement_count),
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

pub(super) fn paint_board_row(
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
        crate::ui::theme::SURFACE_ELEVATED
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
        fill,
        if active {
            crate::ui::theme::stroke_subtle()
        } else {
            egui::Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    if active {
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + crate::ui::theme::SPACE_2),
            egui::pos2(rect.left() + 2.0, rect.bottom() - crate::ui::theme::SPACE_2),
        );
        ui.painter().rect_filled(
            rail_rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_MICRO),
            crate::ui::theme::ACCENT,
        );
    }

    let swatch_size = 24.0;
    let swatch_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + crate::ui::theme::SPACE_3,
            rect.center().y - swatch_size * 0.5,
        ),
        egui::vec2(swatch_size, swatch_size),
    );
    ui.painter().rect_filled(
        swatch_rect,
        egui::CornerRadius::same(crate::ui::theme::RADIUS_INLINE),
        egui::Color32::from_white_alpha(if active { 38 } else { 18 }),
    );

    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let detail_galley =
        ui.painter()
            .layout_no_wrap(detail.to_owned(), detail_font, crate::ui::theme::TEXT_MUTED);

    let text_x = swatch_rect.right() + crate::ui::theme::SPACE_2;
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
        ui.painter()
            .galley(name_rect.left_top(), title_galley, crate::ui::theme::TEXT);
    }

    let detail_pos = egui::pos2(text_x, rect.top() + 28.0);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::ui::theme::TEXT_MUTED);

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
            rect.center().y - galley.size().y * 0.5,
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
    let dash_stroke = egui::Stroke::new(1.0, crate::ui::theme::BORDER);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
        if response.hovered() {
            crate::ui::theme::SURFACE_ELEVATED
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
        crate::ui::theme::TEXT_SUBTLE,
    );
    let pos = egui::pos2(
        rect.center().x - plus_galley.size().x * 0.5,
        rect.center().y - plus_galley.size().y * 0.5 - 1.0,
    );
    ui.painter()
        .galley(pos, plus_galley, crate::ui::theme::TEXT_SUBTLE);
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
