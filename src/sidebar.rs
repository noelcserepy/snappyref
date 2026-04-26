use eframe::egui;

use crate::app::{SUPPORTED_SHORTCUTS, SupportedShortcut};

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
        .order(egui::Order::Foreground)
        .fixed_pos(sidebar_rect.min)
        .show(ctx, |ui| {
            ui.set_min_size(sidebar_rect.size());
            ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
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
    sidebar_tab: &mut SidebarTab,
    settings: &mut crate::data::WorkspaceSettings,
    canvas: &mut crate::canvas::CanvasState,
    boards: &[crate::data::BoardSummary],
    active_board_id: i64,
) -> SidebarRenderResult {
    if !sidebar_open {
        return SidebarRenderResult {
            settings_changed: false,
            interactive_rect: None,
            board_action: None,
        };
    }

    let mut settings_changed = false;
    let mut board_action = None;

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
                ne: crate::theme::CARD_RADIUS,
                sw: 0,
                se: crate::theme::CARD_RADIUS,
            };
            ui.painter().rect(
                shape_rect,
                radius,
                crate::theme::SURFACE,
                crate::theme::stroke_subtle(),
                egui::StrokeKind::Inside,
            );

            let inner_rect = shape_rect.shrink2(egui::vec2(0.0, 0.0));
            let ui_builder = egui::UiBuilder::new().max_rect(inner_rect);
            ui.scope_builder(ui_builder, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                paint_sidebar_tabs(ui, sidebar_tab);
                ui.add_space(crate::theme::SPACE_2);
                match *sidebar_tab {
                    SidebarTab::Boards => {
                        board_action = paint_sidebar_boards(ui, boards, active_board_id)
                    }
                    SidebarTab::Options => {
                        settings_changed |= paint_sidebar_options(ui, settings, canvas);
                    }
                }
            });
        });

    SidebarRenderResult {
        settings_changed,
        interactive_rect: Some(area_response.response.rect),
        board_action,
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
) -> bool {
    let mut changed = false;
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
                    changed |= paint_settings_section(ui, settings, canvas);
                });
                ui.add_space(crate::theme::SPACE_5);
                paint_sidebar_section_header(ui, "Shortcuts", None);
                paint_supported_shortcuts_section(ui);
                ui.add_space(crate::theme::SPACE_4);
            });
    });
    changed
}

fn paint_sidebar_boards(
    ui: &mut egui::Ui,
    boards: &[crate::data::BoardSummary],
    active_board_id: i64,
) -> Option<SidebarBoardAction> {
    let mut board_action = None;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("sidebar_boards_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);
                ui.add_space(crate::theme::SPACE_2);
                let indent = crate::theme::SPACE_2;
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        egui::pos2(ui.max_rect().left() + indent, ui.max_rect().top()),
                        egui::pos2(ui.max_rect().right() - indent, ui.max_rect().bottom()),
                    )),
                    |ui| {
                        for (index, summary) in boards.iter().enumerate() {
                            let shortcut = board_shortcut_hint(index);
                            match paint_board_row(
                                ui,
                                summary.board.id,
                                &summary.board.name,
                                &image_count_label(summary.image_count),
                                summary.board.id == active_board_id,
                                shortcut.as_deref(),
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
                        ui.add_space(crate::theme::SPACE_2);
                        if paint_create_board_button(ui) {
                            board_action = Some(SidebarBoardAction::CreateBoard);
                        }
                        ui.add_space(crate::theme::SPACE_3);
                        ui.label(
                            egui::RichText::new(
                                "Boards help you split your canvas into focused collections.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::theme::TEXT_SUBTLE),
                        );
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
) -> Option<BoardRowAction> {
    let height = 36.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let fill = if active {
        crate::theme::SURFACE_ELEVATED
    } else if response.hovered() {
        crate::theme::SURFACE_ELEVATED
    } else {
        crate::theme::SURFACE
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), fill);

    let swatch_size = 22.0;
    let swatch_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + crate::theme::SPACE_2,
            rect.center().y - swatch_size * 0.5,
        ),
        egui::vec2(swatch_size, swatch_size),
    );
    ui.painter().rect_filled(
        swatch_rect,
        egui::CornerRadius::same(3),
        crate::theme::BORDER,
    );

    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let detail_galley =
        ui.painter()
            .layout_no_wrap(detail.to_owned(), detail_font, crate::theme::TEXT_SUBTLE);

    let text_x = swatch_rect.right() + crate::theme::SPACE_2;
    let delete_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 12.0, rect.center().y),
        egui::vec2(18.0, 18.0),
    );
    let shortcut_reserved_width = if shortcut.is_some() { 42.0 } else { 0.0 };
    let text_right = delete_rect.left() - crate::theme::SPACE_1 - shortcut_reserved_width;
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(text_x, rect.top() + 2.0),
        egui::pos2(text_right.max(text_x + 24.0), rect.top() + 20.0),
    );
    let name_edit_id = ui.make_persistent_id(("board-name-edit", board_id));
    let mut name_edit = ui
        .data_mut(|data| data.get_persisted::<String>(name_edit_id))
        .unwrap_or_else(|| name.to_owned());
    if !ui.memory(|memory| memory.has_focus(name_edit_id)) {
        name_edit = name.to_owned();
    }
    let name_response = ui.put(
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

    let detail_pos = egui::pos2(text_x, rect.center().y + 1.0);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::theme::TEXT_SUBTLE);

    let active_dot_rect = egui::Rect::from_center_size(
        egui::pos2(
            delete_rect.left() - crate::theme::SPACE_1 - 3.0,
            rect.center().y,
        ),
        egui::vec2(5.0, 5.0),
    );
    if active {
        ui.painter().rect_filled(
            active_dot_rect,
            egui::CornerRadius::same(3),
            crate::theme::ACCENT,
        );
    }
    if let Some(shortcut) = shortcut {
        let font = ui.style().text_styles[&egui::TextStyle::Small].clone();
        let galley = ui.painter().layout_no_wrap(
            display_shortcut_label(shortcut).to_owned(),
            font,
            crate::theme::TEXT_SUBTLE,
        );
        let right = if active {
            active_dot_rect.left() - crate::theme::SPACE_1
        } else {
            delete_rect.left() - crate::theme::SPACE_1
        };
        let pos = egui::pos2(
            right - galley.size().x,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(pos, galley, crate::theme::TEXT_SUBTLE);
    }
    let delete_response = ui.put(
        delete_rect,
        egui::Button::new(egui::RichText::new("x").color(crate::theme::TEXT_SUBTLE))
            .small()
            .frame(false),
    );
    if delete_response.clicked() {
        Some(BoardRowAction::Delete)
    } else if rename_submitted {
        Some(BoardRowAction::Rename(name_edit))
    } else if response.clicked() && !name_response.clicked() {
        Some(BoardRowAction::Switch)
    } else {
        None
    }
}

fn paint_create_board_button(ui: &mut egui::Ui) -> bool {
    let height = 30.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let dash_stroke = egui::Stroke::new(1.0, crate::theme::BORDER);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(5),
        if response.hovered() {
            crate::theme::SURFACE_ELEVATED
        } else {
            egui::Color32::TRANSPARENT
        },
        dash_stroke,
        egui::StrokeKind::Inside,
    );

    let plus_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let plus_galley = ui.painter().layout_no_wrap(
        "+  Create new board".to_owned(),
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
    match index {
        0..=8 => Some(format!("Cmd+{}", index + 1)),
        9 => Some("Cmd+0".to_owned()),
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
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            .corner_radius(egui::CornerRadius::same(7))
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
        "Canvas image resolution",
        "Keeps originals for crop/edit.",
        |ui| paint_canvas_image_mode_control(ui, &mut settings.canvas_image_mode),
    );
    paint_setting_divider(ui);
    let downscale_enabled = settings.canvas_image_mode == crate::data::CanvasImageMode::Downscale;
    changed |= paint_setting_row(
        ui,
        "Default image area",
        "Average target side for new imports.",
        |ui| {
            ui.add_enabled_ui(downscale_enabled, |ui| {
                ui.add(
                    egui::DragValue::new(&mut settings.canvas_image_max_side)
                        .speed(64.0)
                        .range(256..=8192)
                        .fixed_decimals(0)
                        .suffix(" px"),
                )
                .changed()
            })
            .inner
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
    paint_setting_divider(ui);
    changed |= paint_setting_row(
        ui,
        "Semantic search",
        #[cfg(feature = "semantic-search")]
        "Experimental; downloads a model on first use.",
        #[cfg(not(feature = "semantic-search"))]
        "Feature disabled in this build.",
        |ui| {
            #[cfg(feature = "semantic-search")]
            {
                paint_switch(ui, &mut settings.semantic_search_enabled).changed()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                ui.add_enabled(
                    false,
                    egui::widgets::Checkbox::without_text(&mut settings.semantic_search_enabled),
                )
                .changed()
            }
        },
    );

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
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_3;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed |= add_control(ui);
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
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
            });
        });
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

fn paint_canvas_image_mode_control(
    ui: &mut egui::Ui,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.x = crate::theme::SPACE_3;
    let downscale_clicked = paint_radio_pill(
        ui,
        "Downscale",
        *canvas_image_mode == crate::data::CanvasImageMode::Downscale,
    );
    if downscale_clicked {
        *canvas_image_mode = crate::data::CanvasImageMode::Downscale;
        changed = true;
    }
    let original_clicked = paint_radio_pill(
        ui,
        "Original",
        *canvas_image_mode == crate::data::CanvasImageMode::Original,
    );
    if original_clicked {
        *canvas_image_mode = crate::data::CanvasImageMode::Original;
        changed = true;
    }
    changed
}

fn paint_radio_pill(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, crate::theme::TEXT);
    let circle_size = 11.0;
    let gap = crate::theme::SPACE_1 + 1.0;
    let total = egui::vec2(circle_size + gap + galley.size().x, 16.0);
    let (rect, response) = ui.allocate_exact_size(total, egui::Sense::click());
    let circle_center = egui::pos2(rect.left() + circle_size * 0.5, rect.center().y);
    let stroke_color = egui::Color32::from_white_alpha(90);
    ui.painter().circle_stroke(
        circle_center,
        circle_size * 0.5 - 0.7,
        egui::Stroke::new(1.4, stroke_color),
    );
    if selected {
        ui.painter()
            .circle_filled(circle_center, 2.5, crate::theme::ACCENT);
    }
    let label_pos = egui::pos2(
        circle_center.x + circle_size * 0.5 + gap,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(label_pos, galley, crate::theme::TEXT);
    response.clicked()
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

    let font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let h_pad = crate::theme::SPACE_3;
    let v_pad = 4.0;
    let seg_gap = 0.0;
    let outer_pad = 2.0;

    let mut seg_widths = [0.0_f32; 3];
    let mut total_w = 0.0_f32;
    for (i, (label, _)) in segments.iter().enumerate() {
        let galley =
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), crate::theme::TEXT);
        seg_widths[i] = galley.size().x + h_pad * 2.0;
        total_w += seg_widths[i];
    }
    total_w += seg_gap * 2.0 + outer_pad * 2.0;
    let inner_h = font.size + v_pad * 2.0;
    let outer_h = inner_h + outer_pad * 2.0;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, outer_h), egui::Sense::hover());
    let container_bg = egui::Color32::from_black_alpha(64);
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(5),
        container_bg,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );

    let mut cursor_x = rect.left() + outer_pad;
    for (i, (label, value)) in segments.iter().enumerate() {
        let seg_rect = egui::Rect::from_min_size(
            egui::pos2(cursor_x, rect.top() + outer_pad),
            egui::vec2(seg_widths[i], inner_h),
        );
        let selected = background == *value;
        let response = ui.interact(
            seg_rect,
            egui::Id::new(("sidebar_bg_segment", *label)),
            egui::Sense::click(),
        );
        if selected {
            ui.painter().rect_filled(
                seg_rect,
                egui::CornerRadius::same(3),
                egui::Color32::from_white_alpha(20),
            );
        } else if response.hovered() {
            ui.painter().rect_filled(
                seg_rect,
                egui::CornerRadius::same(3),
                egui::Color32::from_white_alpha(8),
            );
        }
        let text_color = if selected {
            crate::theme::TEXT
        } else {
            crate::theme::TEXT_MUTED
        };
        let galley = ui
            .painter()
            .layout_no_wrap((*label).to_owned(), font.clone(), text_color);
        let text_pos = egui::pos2(
            seg_rect.center().x - galley.size().x * 0.5,
            seg_rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(text_pos, galley, text_color);
        if response.clicked() {
            background = *value;
        }
        cursor_x += seg_widths[i] + seg_gap;
    }

    if background != before {
        canvas.set_background(background);
        true
    } else {
        false
    }
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

        let (core_shortcuts, layout_shortcuts) = SUPPORTED_SHORTCUTS.split_at(8);
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            paint_shortcut_pill(ui, keys);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
    let pad = egui::vec2(6.0, 2.0);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(4),
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
    let desired_size = egui::vec2(40.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }

    let track_color = if *value {
        crate::theme::ACCENT
    } else {
        crate::theme::SURFACE_ELEVATED
    };
    let knob_color = if *value {
        crate::theme::TEXT
    } else {
        crate::theme::TEXT_MUTED
    };
    let animation = ui.ctx().animate_bool(response.id, *value);
    let knob_radius = 9.0;
    let knob_x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), animation);
    let track_rect = rect.shrink2(egui::vec2(1.0, 3.0));
    let stroke_color = if response.hovered() {
        crate::theme::BORDER_STRONG
    } else {
        crate::theme::BORDER
    };

    ui.painter().rect(
        track_rect,
        track_rect.height() * 0.5,
        track_color,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Middle,
    );
    ui.painter()
        .circle_filled(egui::pos2(knob_x, rect.center().y), knob_radius, knob_color);

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
                .corner_radius(egui::CornerRadius::same(6))
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
                    clicked = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(label)
                                    .text_style(egui::TextStyle::Small)
                                    .color(crate::theme::TEXT_MUTED),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .clicked();
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
    #[cfg(target_os = "macos")]
    {
        "⌘B"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+B"
    }
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
    use super::{display_shortcut_label, shortcut_label_uses_small_symbol_glyphs};

    #[test]
    fn display_shortcut_label_spaces_compact_modifier_chords() {
        assert_eq!(display_shortcut_label("⌘B"), "⌘ B");
        assert_eq!(display_shortcut_label("⇧⌘L"), "⇧ ⌘ L");
        assert_eq!(display_shortcut_label("Ctrl+B"), "Ctrl B");
        assert_eq!(display_shortcut_label("Ctrl+Shift+L"), "Ctrl Shift L");
        assert_eq!(display_shortcut_label("C + Drag"), "C + Drag");
    }

    #[test]
    fn shortcut_label_detects_visually_small_symbol_glyphs() {
        assert!(shortcut_label_uses_small_symbol_glyphs("⇧ ⌘ L"));
        assert!(shortcut_label_uses_small_symbol_glyphs("↵"));
        assert!(!shortcut_label_uses_small_symbol_glyphs("⌘ B"));
    }
}
