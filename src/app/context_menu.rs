//! Right-click context menu over the canvas.

use eframe::egui::{self, Align2, Color32, FontId, Rect, Stroke, Vec2, vec2};

use super::SnappyRefApp;
use super::shortcuts::{self, ContextMenuAction, ContextMenuItem};
use crate::ui::{card, motion, shortcut, theme};

const MENU_WIDTH: f32 = 224.0;
/// Menu rows run denser than card controls so eleven actions stay compact.
const ROW_HEIGHT: f32 = 26.0;

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

/// A menu row: icon, label, and key chips. The hovered row lifts and takes a
/// gold tick on its left edge; disabled rows fade like the Group card's.
pub(super) fn context_menu_row(ui: &mut egui::Ui, item: &ContextMenuItem) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), ROW_HEIGHT),
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
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(card::fade(item.enabled));
    if hover > 0.0 {
        painter.rect_filled(
            rect,
            theme::RADIUS_CONTROL,
            theme::SURFACE_ELEVATED.gamma_multiply(hover),
        );
        let tick =
            Rect::from_center_size(rect.left_center() + vec2(1.5, 0.0), vec2(3.0, 12.0 * hover));
        painter.rect_filled(tick, 1.5, theme::ACCENT.gamma_multiply(hover));
    }

    let quiet = motion::mix(theme::TEXT_MUTED, theme::TEXT, hover);
    let (icon_color, label_color) = if item.action.is_destructive() {
        (theme::DESTRUCTIVE, theme::DESTRUCTIVE)
    } else {
        (quiet, theme::TEXT)
    };
    paint_icon(
        &painter,
        rect.left_center() + vec2(16.0, 0.0),
        item.action,
        icon_color,
    );
    painter.text(
        rect.left_center() + vec2(32.0, 0.0),
        Align2::LEFT_CENTER,
        item.label,
        FontId::proportional(theme::TYPE_CONTROL),
        label_color,
    );
    if let Some(keys) = item.shortcut {
        shortcut::paint(
            &painter,
            rect.right_center() - vec2(6.0, 0.0),
            Align2::RIGHT_CENTER,
            keys,
            quiet,
        );
    }

    response
}

impl SnappyRefApp {
    /// The menu's rows in order, split where a separator falls: board edits,
    /// grouping, stacking, then the library.
    pub(super) fn context_menu_sections(&self) -> [Vec<ContextMenuItem>; 4] {
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
        let item = |action, label, shortcut, enabled| ContextMenuItem {
            action,
            label,
            shortcut,
            enabled,
        };

        [
            vec![
                item(
                    ContextMenuAction::Copy,
                    "Copy",
                    Some(shortcuts::COPY),
                    has_selection,
                ),
                item(
                    ContextMenuAction::Paste,
                    "Paste",
                    Some(shortcuts::PASTE),
                    true,
                ),
                item(
                    ContextMenuAction::Duplicate,
                    "Duplicate",
                    Some(shortcuts::DUPLICATE),
                    has_selection,
                ),
                item(
                    ContextMenuAction::DeleteFromBoard,
                    "Delete",
                    Some(shortcuts::DELETE),
                    has_selection,
                ),
            ],
            vec![
                item(
                    ContextMenuAction::Group,
                    "Group",
                    Some(shortcuts::GROUP),
                    has_group_member,
                ),
                item(
                    ContextMenuAction::Ungroup,
                    "Ungroup",
                    Some(shortcuts::UNGROUP),
                    !self.canvas.selected_group_ids().is_empty(),
                ),
                item(
                    ContextMenuAction::AddToGroup,
                    "Add to group…",
                    Some(shortcuts::ADD_TO_GROUP),
                    has_group_member,
                ),
            ],
            vec![
                item(
                    ContextMenuAction::BringToFront,
                    "Bring to front",
                    Some(shortcuts::BRING_TO_FRONT),
                    has_selection,
                ),
                item(
                    ContextMenuAction::SendToBack,
                    "Send to back",
                    Some(shortcuts::SEND_TO_BACK),
                    has_selection,
                ),
            ],
            vec![
                item(
                    ContextMenuAction::SearchUsingImage,
                    "Search using image",
                    Some(shortcuts::SEARCH_IMAGES),
                    exactly_one_image_only,
                ),
                item(
                    ContextMenuAction::RemoveFromLibrary,
                    "Remove from library…",
                    None,
                    has_selected_image,
                ),
            ],
        ]
    }

    pub(super) fn handle_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        ctx: &egui::Context,
        screen_rect: egui::Rect,
    ) {
        let Some(item) = self
            .context_menu_sections()
            .into_iter()
            .flatten()
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
            ContextMenuAction::BringToFront => self.bring_selection_to_front(),
            ContextMenuAction::SendToBack => self.send_selection_to_back(),
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
        let id = egui::Id::new("board_context_menu");
        let entrance = motion::entrance_on_appear(ctx, id);
        let settle = motion::settle_offset(entrance);
        // Open up or left of the pointer where the menu would cross the
        // window edge. egui sizes the area unseen on its first frame.
        let size = egui::AreaState::load(ctx, id)
            .and_then(|state| state.size)
            .unwrap_or_default();
        let window = ctx.content_rect();
        let side = |fits: bool| {
            if fits {
                egui::Align::Min
            } else {
                egui::Align::Max
            }
        };
        let pivot = Align2([
            side(pos.x + size.x <= window.right()),
            side(pos.y + size.y <= window.bottom()),
        ]);

        let mut close_menu = false;
        let menu_response = egui::Area::new(id)
            .order(egui::Order::Tooltip)
            .pivot(pivot)
            .fixed_pos(pos - vec2(0.0, settle))
            .show(ctx, |ui| {
                ui.set_opacity(entrance.clamp(0.0, 1.0));
                card::frame()
                    .corner_radius(theme::RADIUS_PANEL)
                    .inner_margin(theme::SPACE_1)
                    .show(ui, |ui| {
                        ui.set_width(MENU_WIDTH);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (index, section) in self.context_menu_sections().into_iter().enumerate()
                        {
                            if index > 0 {
                                card::separator(ui);
                            }
                            for item in section {
                                if context_menu_row(ui, &item).clicked() {
                                    self.handle_context_menu_action(item.action, ctx, screen_rect);
                                    close_menu = true;
                                }
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

/// Row icons, 14 points on the Group card's 16 unit icon grid.
fn paint_icon(p: &egui::Painter, center: egui::Pos2, action: ContextMenuAction, color: Color32) {
    use ContextMenuAction as A;
    let size = 14.0;
    let origin = center - Vec2::splat(size * 0.5);
    let point = |x: f32, y: f32| origin + vec2(x, y) * (size / 16.0);
    let stroke = Stroke::new(1.25, color);
    let line = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let rect = |x: f32, y: f32, w: f32, h: f32, r: f32| {
        p.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            r,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    let dashed = |points: &[(f32, f32)]| {
        p.extend(egui::Shape::dashed_line(
            &points.iter().map(|&(x, y)| point(x, y)).collect::<Vec<_>>(),
            stroke,
            1.75,
            1.3,
        ));
    };
    match action {
        A::Copy => {
            rect(5.5, 5.5, 9.0, 9.0, 1.5);
            line(&[(2.0, 10.5), (2.0, 2.0), (10.5, 2.0)]);
        }
        A::Paste => {
            rect(2.5, 3.0, 11.0, 11.5, 1.5);
            rect(5.5, 1.5, 5.0, 3.0, 1.0);
        }
        A::Duplicate => {
            rect(1.5, 1.5, 8.5, 8.5, 1.5);
            rect(6.0, 6.0, 8.5, 8.5, 1.5);
        }
        A::DeleteFromBoard => {
            line(&[(2.0, 4.0), (14.0, 4.0)]);
            line(&[(6.0, 4.0), (6.5, 2.0), (9.5, 2.0), (10.0, 4.0)]);
            line(&[(3.5, 4.0), (4.5, 14.0), (11.5, 14.0), (12.5, 4.0)]);
        }
        // Two members set diagonally, so the frame never reads as a pause glyph.
        A::Group => {
            rect(1.5, 1.5, 13.0, 13.0, 2.0);
            rect(4.25, 4.25, 4.0, 4.0, 0.0);
            rect(7.75, 7.75, 4.0, 4.0, 0.0);
        }
        A::Ungroup => {
            dashed(&[
                (1.5, 1.5),
                (14.5, 1.5),
                (14.5, 14.5),
                (1.5, 14.5),
                (1.5, 1.5),
            ]);
            rect(4.25, 4.25, 4.0, 4.0, 0.0);
            rect(7.75, 7.75, 4.0, 4.0, 0.0);
        }
        A::AddToGroup => {
            rect(1.5, 1.5, 13.0, 13.0, 2.0);
            line(&[(8.0, 5.0), (8.0, 11.0)]);
            line(&[(5.0, 8.0), (11.0, 8.0)]);
        }
        A::BringToFront => {
            line(&[(3.0, 2.5), (13.0, 2.5)]);
            line(&[(8.0, 14.0), (8.0, 6.0)]);
            line(&[(4.5, 9.5), (8.0, 6.0), (11.5, 9.5)]);
        }
        A::SendToBack => {
            line(&[(3.0, 13.5), (13.0, 13.5)]);
            line(&[(8.0, 2.0), (8.0, 10.0)]);
            line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)]);
        }
        A::SearchUsingImage => {
            p.circle_stroke(point(7.0, 7.0), 4.75 * size / 16.0, stroke);
            line(&[(10.5, 10.5), (14.0, 14.0)]);
        }
        A::RemoveFromLibrary => {
            line(&[(3.5, 1.75), (12.5, 1.75)]);
            rect(1.5, 4.0, 13.0, 10.5, 1.5);
            line(&[(5.5, 9.25), (10.5, 9.25)]);
        }
    }
}
