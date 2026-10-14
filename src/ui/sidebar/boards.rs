//! The Boards tab: every board as a row with the digit that switches to it,
//! then the row that makes a new one.

use eframe::egui::{self, Align2, FontId};

use super::SidebarBoardAction;
use crate::app::{BOARD_DIGITS, NEW_BOARD, SWITCH_BOARD};
use crate::ui::shortcut::{self, Key};
use crate::ui::{card, theme};

const ROW_HEIGHT: f32 = 30.0;
/// The digit cap's center and the name's start, from the row's left.
const CAP_X: f32 = 17.0;
const NAME_X: f32 = 32.0;
/// The count's and Delete's right edge, from the row's right.
const TRAILING_X: f32 = 10.0;
/// Only one board renames at a time, so one field serves every row.
const RENAME_FIELD: &str = "Board name";

pub(super) fn show(
    ui: &mut egui::Ui,
    boards: &[crate::data::BoardSummary],
    active_board_id: i64,
) -> Option<SidebarBoardAction> {
    super::column(ui, 18.0, |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(theme::editorial_caption("All boards"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                shortcut::show(ui, SWITCH_BOARD);
            });
        });
    });
    let mut action = None;
    super::scroll(ui, "sidebar_boards_scroll", |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (index, summary) in boards.iter().enumerate() {
            let board = &summary.board;
            let row = board_row(
                ui,
                board.id,
                &board.name,
                summary.placement_count,
                board.id == active_board_id,
                BOARD_DIGITS.get(index).copied(),
            );
            action = action.take().or(row);
        }
        ui.add_space(6.0);
        if new_board_row(ui) {
            action = Some(SidebarBoardAction::Create);
        }
    });
    action
}

/// A board row allocates a fixed rect and paints its states over it, so
/// hover, Delete, and renaming never shift the list.
fn board_row(
    ui: &mut egui::Ui,
    id: i64,
    name: &str,
    count: usize,
    active: bool,
    digit: Option<Key>,
) -> Option<SidebarBoardAction> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    let renaming_key = egui::Id::new("board-renaming");
    let renaming = ui.data(|d| d.get_temp::<i64>(renaming_key)) == Some(id);
    let hot = renaming || ui.rect_contains_pointer(rect);
    let fill = if active {
        theme::SURFACE_ELEVATED
    } else if hot {
        theme::SURFACE_PRESSED
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, theme::RADIUS_CONTROL, fill);
    if let Some(digit) = digit {
        shortcut::cap(
            ui.painter(),
            rect.left_center() + egui::vec2(CAP_X, 0.0),
            digit,
            active,
        );
    }

    let detail = FontId::proportional(theme::TYPE_DETAIL);
    let trailing = egui::pos2(rect.right() - TRAILING_X, rect.center().y);
    let delete_rect = Align2::RIGHT_CENTER.anchor_size(
        trailing + egui::vec2(4.0, 0.0),
        egui::vec2(48.0, ROW_HEIGHT - 6.0),
    );
    let delete = ui.interact(
        delete_rect,
        ui.id().with(("board-delete", id)),
        egui::Sense::click(),
    );
    delete.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Delete board {name}"),
        )
    });
    let trailing_rect = if hot {
        let ink = if delete.hovered() {
            theme::DESTRUCTIVE
        } else {
            theme::DESTRUCTIVE.gamma_multiply(0.75)
        };
        ui.painter()
            .text(trailing, Align2::RIGHT_CENTER, "Delete", detail, ink)
    } else {
        let ink = if active {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_SUBTLE
        };
        ui.painter().text(
            trailing,
            Align2::RIGHT_CENTER,
            count.to_string(),
            detail,
            ink,
        )
    };

    let name_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + NAME_X, rect.top()),
        egui::pos2(trailing_rect.left() - 10.0, rect.bottom()),
    );
    let name_hit = ui.interact(
        name_rect,
        ui.id().with(("board-name", id)),
        egui::Sense::click(),
    );
    name_hit.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Board {name}"),
        )
    });
    let double_clicked = name_hit.double_clicked();
    if double_clicked {
        ui.data_mut(|d| d.insert_temp(renaming_key, id));
    }
    let mut renamed = None;
    if renaming || double_clicked {
        let (submitted, focused) = ui
            .scope_builder(egui::UiBuilder::new().max_rect(name_rect), |ui| {
                card::field(
                    ui,
                    RENAME_FIELD,
                    name,
                    name_rect.width(),
                    ROW_HEIGHT,
                    true,
                    theme::TYPE_CONTROL,
                    egui::Align::Min,
                )
            })
            .inner;
        // Focus after the field runs: on this frame's press the field would
        // treat the double-click as a click elsewhere and drop focus.
        if double_clicked {
            card::focus_field(ui.ctx(), RENAME_FIELD);
        }
        renamed = submitted.filter(|n| !n.trim().is_empty());
        if !focused && !double_clicked {
            ui.data_mut(|d| d.remove::<i64>(renaming_key));
        }
    } else {
        let ink = if active || hot {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        };
        let mut job = egui::text::LayoutJob::single_section(
            name.to_owned(),
            egui::TextFormat::simple(FontId::proportional(theme::TYPE_CONTROL), ink),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width());
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(name_rect.left(), rect.center().y - galley.size().y * 0.5),
            galley,
            ink,
        );
    }

    if delete.clicked() {
        Some(SidebarBoardAction::RequestDelete(id))
    } else if let Some(name) = renamed {
        Some(SidebarBoardAction::Rename(id, name))
    } else if !double_clicked && (response.clicked() || name_hit.clicked()) {
        Some(SidebarBoardAction::Switch(id))
    } else {
        None
    }
}

fn new_board_row(ui: &mut egui::Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "New board")
    });
    let hot = response.hovered();
    if hot {
        ui.painter()
            .rect_filled(rect, theme::RADIUS_CONTROL, theme::SURFACE_PRESSED);
    }
    let p = ui.painter();
    shortcut::cap(
        p,
        rect.left_center() + egui::vec2(CAP_X, 0.0),
        Key::Named("+"),
        false,
    );
    p.text(
        rect.left_center() + egui::vec2(NAME_X, 0.0),
        Align2::LEFT_CENTER,
        "New board",
        FontId::proportional(theme::TYPE_CONTROL),
        if hot { theme::TEXT } else { theme::TEXT_MUTED },
    );
    shortcut::paint(
        p,
        rect.right_center() - egui::vec2(TRAILING_X, 0.0),
        Align2::RIGHT_CENTER,
        NEW_BOARD,
        theme::TEXT_MUTED,
    );
    response.clicked()
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    #[test]
    fn double_clicking_a_board_name_submits_the_typed_rename() {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(264.0, 200.0))
            .with_step_dt(1.0 / 60.0)
            .build_ui_state(
                |ui, action| {
                    if let Some(submitted) =
                        board_row(ui, 1, "My Board", 0, true, Some(BOARD_DIGITS[0]))
                    {
                        *action = Some(submitted);
                    }
                },
                None,
            );
        let pos = harness.get_by_label("Board My Board").rect().center();
        harness.hover_at(pos);
        for _ in 0..2 {
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            harness.step();
        }
        harness.step();
        assert!(
            harness.ctx.egui_wants_keyboard_input(),
            "double-click must focus the rename field"
        );
        harness.get_by_label(RENAME_FIELD);
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.step();
        harness.event(egui::Event::Text("References".to_owned()));
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        assert_eq!(
            *harness.state(),
            Some(SidebarBoardAction::Rename(1, "References".to_owned()))
        );
    }
}
