//! Right-click context menu over the canvas.

use eframe::egui;

use super::SnappyRefApp;
use super::shortcuts::{self, ContextMenuAction, ContextMenuItem};
use crate::ui::{card, motion, shortcut, theme};

pub(super) fn context_menu_should_close(
    action_clicked: bool,
    just_opened: bool,
    menu_rect: egui::Rect,
    pointer_pos: Option<egui::Pos2>,
    primary_pressed: bool,
    secondary_pressed: bool,
    escape_pressed: bool,
) -> bool {
    if action_clicked || escape_pressed {
        return true;
    }
    if just_opened || (!primary_pressed && !secondary_pressed) {
        return false;
    }
    pointer_pos.is_none_or(|pos| !menu_rect.contains(pos))
}

/// Menu rows: label left, shortcut chips right, on the card's row metrics.
pub(super) fn context_menu_row(ui: &mut egui::Ui, item: &ContextMenuItem) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::CONTROL_HEIGHT),
        if item.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, item.label)
    });

    let hover = motion::hover_amount(
        ui.ctx(),
        response.id.with("context-menu-row-hover"),
        response.hovered() && item.enabled,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            theme::RADIUS_CONTROL,
            theme::SURFACE_ELEVATED.gamma_multiply(hover),
        );
    }

    let mut painter = ui.painter().clone();
    if !item.enabled {
        painter.multiply_opacity(0.4);
    }
    let text_color = if item.action.is_destructive() {
        theme::DESTRUCTIVE
    } else {
        theme::TEXT
    };
    painter.text(
        rect.left_center() + egui::vec2(theme::SPACE_2, 0.0),
        egui::Align2::LEFT_CENTER,
        item.label,
        egui::FontId::proportional(theme::TYPE_CONTROL),
        text_color,
    );
    if let Some(keys) = item.shortcut {
        shortcut::paint(
            &painter,
            rect.right_center() - egui::vec2(theme::SPACE_2, 0.0),
            egui::Align2::RIGHT_CENTER,
            keys,
            theme::TEXT_MUTED,
        );
    }

    response
}

impl SnappyRefApp {
    pub(super) fn context_menu_items(&self) -> Vec<ContextMenuItem> {
        let selected_item_count = self.canvas.selected_placement_ids().len();
        let selected_annotation_count = self.canvas.selected_annotation_ids().len();
        let has_selection = self.canvas.has_selection();
        let has_selected_image = selected_item_count > 0;
        let has_group_member = has_selected_image
            || self.canvas.selected_annotation_ids().iter().any(|&id| {
                self.canvas
                    .annotation_for_id(id)
                    .is_some_and(|a| a.kind == crate::data::CanvasAnnotationKind::Textbox)
            });
        let exactly_one_image_only = selected_item_count == 1 && selected_annotation_count == 0;

        vec![
            ContextMenuItem {
                action: ContextMenuAction::Group,
                label: "Group",
                shortcut: Some(shortcuts::GROUP),
                enabled: has_group_member,
            },
            ContextMenuItem {
                action: ContextMenuAction::Ungroup,
                label: "Ungroup",
                shortcut: Some(shortcuts::UNGROUP),
                enabled: !self.canvas.selected_group_ids().is_empty(),
            },
            ContextMenuItem {
                action: ContextMenuAction::AddToGroup,
                label: "Add to group",
                shortcut: Some(shortcuts::ADD_TO_GROUP),
                enabled: has_group_member,
            },
            ContextMenuItem {
                action: ContextMenuAction::Copy,
                label: "Copy",
                shortcut: Some(shortcuts::COPY),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::Paste,
                label: "Paste",
                shortcut: Some(shortcuts::PASTE),
                enabled: true,
            },
            ContextMenuItem {
                action: ContextMenuAction::Duplicate,
                label: "Duplicate",
                shortcut: Some(shortcuts::DUPLICATE),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::DeleteFromBoard,
                label: "Delete (from board)",
                shortcut: Some(shortcuts::DELETE),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToFront,
                label: "Move to Front",
                shortcut: Some(shortcuts::MOVE_TO_FRONT),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToBack,
                label: "Move to Back",
                shortcut: Some(shortcuts::MOVE_TO_BACK),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::SearchUsingImage,
                label: "Search using image",
                shortcut: Some(shortcuts::SEARCH_IMAGES),
                enabled: exactly_one_image_only,
            },
            ContextMenuItem {
                action: ContextMenuAction::RemoveFromLibrary,
                label: "Remove from Library...",
                shortcut: None,
                enabled: has_selected_image,
            },
        ]
    }

    pub(super) fn handle_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let Some(item) = self
            .context_menu_items()
            .into_iter()
            .find(|item| item.action == action)
        else {
            return;
        };
        if !item.enabled {
            return;
        }

        match action {
            ContextMenuAction::Group => self.group_selection(),
            ContextMenuAction::Ungroup => self.ungroup_selection(),
            ContextMenuAction::AddToGroup => self.open_group_picker(),
            ContextMenuAction::Copy => self.copy_selection_to_system_clipboard(),
            ContextMenuAction::Paste => self.handle_clipboard_paste_from_context(ctx, screen_rect),
            ContextMenuAction::Duplicate => self.duplicate_selection_with_gap(),
            ContextMenuAction::DeleteFromBoard => {
                self.delete_selected_items_from_board(screen_rect);
            }
            ContextMenuAction::MoveToFront => self.move_selected_canvas_items_to_front(),
            ContextMenuAction::MoveToBack => self.move_selected_canvas_items_to_back(),
            ContextMenuAction::SearchUsingImage => {
                self.search.search_using_selected_image(
                    self.library.as_ref(),
                    &mut self.canvas,
                    &mut self.status,
                    self.settings.canvas_image_max_side,
                );
            }
            ContextMenuAction::RemoveFromLibrary => self.request_selected_assets_library_remove(),
        }
    }

    pub(super) fn show_context_menu(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
        just_opened: bool,
    ) {
        let Some(pos) = self.context_menu_pos else {
            return;
        };
        let entrance = motion::entrance_on_appear(ctx, egui::Id::new("board_context_menu"));
        let settle = motion::settle_offset(entrance);

        let mut close_menu = false;
        let menu_response = egui::Area::new(egui::Id::new("board_context_menu"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos - egui::vec2(0.0, settle))
            .show(ctx, |ui| {
                ui.set_opacity(entrance.clamp(0.0, 1.0));
                card::frame()
                    .corner_radius(theme::RADIUS_PANEL)
                    .inner_margin(theme::SPACE_1)
                    .show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for item in self.context_menu_items() {
                            let response = context_menu_row(ui, &item);
                            if response.clicked() {
                                self.handle_context_menu_action(item.action, ctx, screen_rect);
                                close_menu = true;
                            }
                        }
                    });
            });
        motion::settle_layer_scale(ctx, menu_response.response.layer_id, pos, entrance);

        let should_close = ctx.input(|input| {
            context_menu_should_close(
                close_menu,
                just_opened,
                menu_response.response.rect,
                input.pointer.latest_pos(),
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if should_close {
            self.context_menu_pos = None;
        }
    }

    pub(super) fn handle_clipboard_paste_from_context(
        &mut self,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        let desired_world = self
            .context_menu_pos
            .map(|pos| self.canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| self.preferred_import_world(ctx, screen_rect));
        self.handle_clipboard_paste_at(screen_rect, desired_world, paste_text);
    }
}
