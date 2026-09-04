//! Right-click context menu over the canvas.

use eframe::egui;

use super::SnappyRefApp;
use super::shortcuts::{ContextMenuAction, ContextMenuItem, platform_shortcut_label};

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

pub(super) fn context_menu_row(ui: &mut egui::Ui, item: &ContextMenuItem) -> egui::Response {
    let row_height = 28.0;
    let shortcut_width: f32 = 54.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height),
        if item.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, item.enabled, item.label)
    });

    let hover = crate::ui::motion::hover_amount(
        ui.ctx(),
        response.id.with("context-menu-row-hover"),
        response.hovered() && item.enabled,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT),
            egui::Color32::from_white_alpha((18.0 * hover) as u8),
        );
    }

    let text_color = if item.enabled && item.action.is_destructive() {
        crate::ui::theme::DESTRUCTIVE
    } else if item.enabled {
        crate::ui::theme::TEXT
    } else {
        crate::ui::theme::TEXT_MUTED
    };
    let shortcut_color = if item.enabled {
        crate::ui::theme::TEXT_SUBTLE
    } else {
        crate::ui::theme::TEXT_MUTED
    };

    let label_pos = egui::pos2(rect.left() + 8.0, rect.center().y - 7.5);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_TOP,
        item.label,
        egui::FontId::proportional(13.0),
        text_color,
    );

    if let Some(shortcut) = item.shortcut {
        let pill_size = egui::vec2(shortcut_width.min(rect.width() * 0.35), 20.0);
        let pill_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 8.0 - pill_size.x * 0.5, rect.center().y),
            pill_size,
        );
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            shortcut,
            egui::FontId::monospace(11.0),
            shortcut_color,
        );
    }

    response
}

impl SnappyRefApp {
    pub(super) fn context_menu_items(&self) -> Vec<ContextMenuItem> {
        let selected_item_count = self.canvas.selected_placement_ids().len();
        let selected_annotation_count = self.canvas.selected_annotation_ids().len();
        let has_selection = selected_item_count > 0 || selected_annotation_count > 0;
        let has_selected_image = selected_item_count > 0;
        let exactly_one_image_only = selected_item_count == 1 && selected_annotation_count == 0;

        vec![
            ContextMenuItem {
                action: ContextMenuAction::Copy,
                label: "Copy",
                shortcut: Some(platform_shortcut_label("⌘ C", "Ctrl C")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::Paste,
                label: "Paste",
                shortcut: Some(platform_shortcut_label("⌘ V", "Ctrl V")),
                enabled: true,
            },
            ContextMenuItem {
                action: ContextMenuAction::Duplicate,
                label: "Duplicate",
                shortcut: Some(platform_shortcut_label("⌘ D", "Ctrl D")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::DeleteFromBoard,
                label: "Delete (from board)",
                shortcut: Some("del"),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToFront,
                label: "Move to Front",
                shortcut: Some(platform_shortcut_label("⌘ .", "Ctrl .")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::MoveToBack,
                label: "Move to Back",
                shortcut: Some(platform_shortcut_label("⌘ ,", "Ctrl ,")),
                enabled: has_selection,
            },
            ContextMenuItem {
                action: ContextMenuAction::SearchUsingImage,
                label: "Search using image",
                shortcut: Some(platform_shortcut_label("⌘ F", "Ctrl F")),
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
            ContextMenuAction::Copy => self.copy_selection_to_context_clipboard(ctx),
            ContextMenuAction::Paste => self.handle_clipboard_paste_from_context(ctx, screen_rect),
            ContextMenuAction::Duplicate => self.duplicate_selection_with_gap(),
            ContextMenuAction::DeleteFromBoard => {
                self.delete_selected_items_from_board(screen_rect);
            }
            ContextMenuAction::MoveToFront => self.move_selected_canvas_items_to_front(),
            ContextMenuAction::MoveToBack => self.move_selected_canvas_items_to_back(),
            ContextMenuAction::SearchUsingImage => self.search_using_selected_image(),
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
        let entrance =
            crate::ui::motion::entrance_on_appear(ctx, egui::Id::new("board_context_menu"));
        let settle = crate::ui::motion::settle_offset(entrance);

        let mut close_menu = false;
        let menu_response = egui::Area::new(egui::Id::new("board_context_menu"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos - egui::vec2(0.0, settle))
            .show(ctx, |ui| {
                ui.set_opacity(entrance.clamp(0.0, 1.0));
                egui::Frame::new()
                    .fill(crate::ui::theme::SURFACE)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(crate::ui::theme::RADIUS_COMPACT))
                    .shadow(ui.style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::same(crate::ui::theme::SPACE_1 as i8))
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
        crate::ui::motion::settle_layer_scale(ctx, menu_response.response.layer_id, pos, entrance);

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
        let paste_text = self.context_clipboard_text(ctx);
        let desired_world = self
            .context_menu_pos
            .map(|pos| self.canvas.screen_to_world(pos, screen_rect))
            .unwrap_or_else(|| self.preferred_import_world(ctx, screen_rect));
        self.handle_clipboard_paste_at(screen_rect, desired_world, paste_text);
    }

    pub(super) fn context_clipboard_text(&mut self, ctx: &egui::Context) -> Option<String> {
        let paste_text = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        if paste_text.is_some() {
            return paste_text;
        }

        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(_) => return None,
        };
        clipboard.get_text().ok()
    }
}
