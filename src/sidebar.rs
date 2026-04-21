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
}

#[derive(Clone, Copy)]
struct ShortcutRow {
    action: &'static str,
    windows: &'static str,
    mac: &'static str,
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
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> SidebarRenderResult {
    if !sidebar_open {
        return SidebarRenderResult {
            settings_changed: false,
            interactive_rect: None,
        };
    }

    let mut settings_changed = false;

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
                paint_sidebar_header(ui);
                paint_sidebar_tabs(ui, sidebar_tab);
                ui.add_space(crate::theme::SPACE_2);
                match *sidebar_tab {
                    SidebarTab::Boards => paint_sidebar_boards(ui, canvas),
                    SidebarTab::Options => {
                        settings_changed |= paint_sidebar_options(
                            ui,
                            auto_image_scaling,
                            import_gap,
                            canvas_image_mode,
                            canvas_image_max_side,
                            semantic_search_enabled,
                            canvas,
                        );
                    }
                }
            });
        });

    SidebarRenderResult {
        settings_changed,
        interactive_rect: Some(area_response.response.rect),
    }
}

fn paint_sidebar_header(ui: &mut egui::Ui) {
    let rect = ui
        .allocate_space(egui::vec2(
            ui.available_width(),
            crate::theme::CONTROL_HEIGHT + crate::theme::SPACE_2,
        ))
        .1;
    let pad_x = crate::theme::SPACE_4;
    let center_y = rect.center().y;
    let badge_size = 22.0;
    let badge_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + pad_x + badge_size * 0.5, center_y),
        egui::vec2(badge_size, badge_size),
    );
    ui.painter().rect_filled(
        badge_rect,
        egui::CornerRadius::same(5),
        crate::theme::BADGE,
    );
    let badge_text_color = egui::Color32::from_rgb(0x1A, 0x19, 0x17);
    let badge_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let badge_galley = ui
        .painter()
        .layout_no_wrap("R".to_owned(), badge_font, badge_text_color);
    let badge_pos = egui::pos2(
        badge_rect.center().x - badge_galley.size().x * 0.5,
        badge_rect.center().y - badge_galley.size().y * 0.5,
    );
    ui.painter()
        .galley(badge_pos, badge_galley, badge_text_color);

    let name_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let name_galley = ui.painter().layout_no_wrap(
        "Reference".to_owned(),
        name_font,
        crate::theme::TEXT,
    );
    let name_pos = egui::pos2(
        badge_rect.right() + crate::theme::SPACE_2,
        center_y - name_galley.size().y * 0.5,
    );
    ui.painter().galley(name_pos, name_galley, crate::theme::TEXT);

    let kbd_label = sidebar_shortcut_label();
    let kbd_font = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    let kbd_galley =
        ui.painter()
            .layout_no_wrap(kbd_label.to_owned(), kbd_font, crate::theme::TEXT_SUBTLE);
    let kbd_pad = egui::vec2(crate::theme::SPACE_1 + 1.0, 2.0);
    let kbd_size = kbd_galley.size() + kbd_pad * 2.0;
    let kbd_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - pad_x - kbd_size.x, center_y - kbd_size.y * 0.5),
        kbd_size,
    );
    ui.painter().rect(
        kbd_rect,
        egui::CornerRadius::same(4),
        crate::theme::SURFACE_ELEVATED,
        crate::theme::stroke_subtle(),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        kbd_rect.min + kbd_pad,
        kbd_galley,
        crate::theme::TEXT_SUBTLE,
    );
}

fn paint_sidebar_tabs(ui: &mut egui::Ui, sidebar_tab: &mut SidebarTab) {
    let bar_height = 32.0;
    let (_id, rect) =
        ui.allocate_space(egui::vec2(ui.available_width(), bar_height + 1.0));
    let pad_x = crate::theme::SPACE_3;
    let tab_rect =
        egui::Rect::from_min_max(egui::pos2(rect.left() + pad_x, rect.top()), rect.max);
    let tabs: [(&str, SidebarTab); 2] = [
        ("Boards", SidebarTab::Boards),
        ("Options", SidebarTab::Options),
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
            ui.painter()
                .rect_filled(underline, 0, crate::theme::ACCENT);
        }
    }
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.bottom(),
        crate::theme::stroke_subtle(),
    );
}

fn paint_sidebar_options(
    ui: &mut egui::Ui,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;
    let content_rect = ui.available_rect_before_wrap();
    ui.scope_builder(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_2);
        egui::ScrollArea::vertical()
            .id_salt("sidebar_options_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_3);
                ui.add_space(crate::theme::SPACE_2);
                paint_sidebar_section_header(ui, "Options", Some("Board defaults"));
                paint_sidebar_subcard(ui, |ui| {
                    changed |= paint_settings_section(
                        ui,
                        auto_image_scaling,
                        import_gap,
                        canvas_image_mode,
                        canvas_image_max_side,
                        semantic_search_enabled,
                        canvas,
                    );
                });
                ui.add_space(crate::theme::SPACE_4);
                paint_sidebar_section_header(ui, "Shortcuts", None);
                paint_supported_shortcuts_section(ui);
                ui.add_space(crate::theme::SPACE_4);
            });
    });
    changed
}

fn paint_sidebar_boards(ui: &mut egui::Ui, _canvas: &mut crate::canvas::CanvasState) {
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
                        paint_board_row(
                            ui,
                            "Current board",
                            "All images",
                            true,
                        );
                        ui.add_space(crate::theme::SPACE_2);
                        paint_create_board_button(ui);
                        ui.add_space(crate::theme::SPACE_3);
                        ui.label(
                            egui::RichText::new("Boards help you split your canvas into focused collections.")
                                .text_style(egui::TextStyle::Small)
                                .color(crate::theme::TEXT_SUBTLE),
                        );
                    },
                );
            });
    });
}

fn paint_board_row(ui: &mut egui::Ui, name: &str, detail: &str, active: bool) {
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

    let name_font = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let detail_font = ui.style().text_styles[&egui::TextStyle::Small].clone();
    let name_galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), name_font, crate::theme::TEXT);
    let detail_galley = ui.painter().layout_no_wrap(
        detail.to_owned(),
        detail_font,
        crate::theme::TEXT_SUBTLE,
    );

    let text_x = swatch_rect.right() + crate::theme::SPACE_2;
    let name_pos = egui::pos2(text_x, rect.center().y - name_galley.size().y - 1.0);
    let detail_pos = egui::pos2(text_x, rect.center().y + 1.0);
    ui.painter().galley(name_pos, name_galley, crate::theme::TEXT);
    ui.painter()
        .galley(detail_pos, detail_galley, crate::theme::TEXT_SUBTLE);

    if active {
        let dot = egui::Rect::from_center_size(
            egui::pos2(rect.right() - crate::theme::SPACE_3, rect.center().y),
            egui::vec2(5.0, 5.0),
        );
        ui.painter()
            .rect_filled(dot, egui::CornerRadius::same(3), crate::theme::ACCENT);
    }
}

fn paint_create_board_button(ui: &mut egui::Ui) {
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
    ui.add_space(22.0);
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
                top: crate::theme::SPACE_2 as i8,
                bottom: crate::theme::SPACE_2 as i8,
            })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing =
                    egui::vec2(crate::theme::SPACE_2, crate::theme::SPACE_1);
                add_contents(ui);
            });
    });
}

fn paint_settings_section(
    ui: &mut egui::Ui,
    auto_image_scaling: &mut bool,
    import_gap: &mut f32,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
    canvas_image_max_side: &mut u32,
    semantic_search_enabled: &mut bool,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);

        paint_sidebar_subsection_label(ui, "Canvas");
        ui.add_space(crate::theme::SPACE_1);

        changed |= paint_setting_row(ui, "Canvas background", "Quiet board guide.", |ui| {
            paint_background_mode_control(ui, canvas)
        });
        changed |= paint_setting_row(
            ui,
            "Canvas image resolution",
            "Keeps originals for crop/edit.",
            |ui| paint_canvas_image_mode_control(ui, canvas_image_mode),
        );
        changed |= paint_setting_row(
            ui,
            "Canvas max dimension",
            "Largest side of the canvas copy.",
            |ui| {
                ui.add_enabled_ui(
                    *canvas_image_mode == crate::data::CanvasImageMode::Downscale,
                    |ui| {
                        ui.add(
                            egui::DragValue::new(canvas_image_max_side)
                                .speed(64.0)
                                .range(256..=8192)
                                .fixed_decimals(0)
                                .suffix(" px"),
                        )
                        .changed()
                    },
                )
                .inner
            },
        );

        ui.add_space(crate::theme::SPACE_1);
        paint_sidebar_subsection_label(ui, "Import + search");
        ui.add_space(crate::theme::SPACE_1);

        let auto_scale_detail = "Caps new imports to a default area.";
        changed |= paint_setting_row(ui, "Auto-scale new images", auto_scale_detail, |ui| {
            #[cfg(feature = "semantic-search")]
            {
                paint_switch(ui, auto_image_scaling).changed()
            }
            #[cfg(not(feature = "semantic-search"))]
            {
                ui.add_enabled(
                    false,
                    egui::widgets::Checkbox::without_text(auto_image_scaling),
                )
                .changed()
            }
        });
        changed |= paint_setting_row(
            ui,
            "Import gap",
            "Gap used when placing new images.",
            |ui| {
                ui.add(
                    egui::DragValue::new(import_gap)
                        .speed(0.5)
                        .range(0.0..=128.0)
                        .fixed_decimals(0)
                        .suffix(" units"),
                )
                .changed()
            },
        );
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
                    paint_switch(ui, semantic_search_enabled).changed()
                }
                #[cfg(not(feature = "semantic-search"))]
                {
                    ui.add_enabled(
                        false,
                        egui::widgets::Checkbox::without_text(semantic_search_enabled),
                    )
                    .changed()
                }
            },
        );
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
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            egui::RichText::new(title)
                .text_style(egui::TextStyle::Body)
                .strong()
                .color(crate::theme::TEXT),
        );
        ui.label(
            egui::RichText::new(detail)
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_MUTED),
        );
        ui.add_space(crate::theme::SPACE_1);
        changed |= add_control(ui);
        ui.add_space(crate::theme::SPACE_2);
    });
    changed
}

fn paint_canvas_image_mode_control(
    ui: &mut egui::Ui,
    canvas_image_mode: &mut crate::data::CanvasImageMode,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .radio_value(
                canvas_image_mode,
                crate::data::CanvasImageMode::Downscale,
                "Downscale",
            )
            .changed();
        changed |= ui
            .radio_value(
                canvas_image_mode,
                crate::data::CanvasImageMode::Original,
                "Original",
            )
            .changed();
    });
    changed
}

fn paint_background_mode_control(
    ui: &mut egui::Ui,
    canvas: &mut crate::canvas::CanvasState,
) -> bool {
    let mut background = canvas.background();
    let before = background;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = crate::theme::SPACE_1;
        for (label, value) in [
            ("Dots", crate::data::CanvasBackground::Dots),
            ("Grid", crate::data::CanvasBackground::Grid),
            ("None", crate::data::CanvasBackground::None),
        ] {
            let selected = background == value;
            let button = egui::Button::new(
                egui::RichText::new(label)
                    .text_style(egui::TextStyle::Button)
                    .color(crate::theme::TEXT),
            )
            .fill(if selected {
                crate::theme::SURFACE_ELEVATED
            } else {
                crate::theme::SURFACE
            })
            .stroke(if selected {
                crate::theme::stroke_strong()
            } else {
                crate::theme::stroke_subtle()
            })
            .corner_radius(crate::theme::CONTROL_RADIUS)
            .min_size(egui::vec2(44.0, crate::theme::CONTROL_HEIGHT));

            if ui.add(button).clicked() {
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

fn paint_supported_shortcuts_section(ui: &mut egui::Ui) {
    paint_sidebar_subsection_label(ui, "Implemented");
    ui.add_space(crate::theme::SPACE_1);
    ui.label(
        egui::RichText::new("Bindings grouped by task.")
            .text_style(egui::TextStyle::Small)
            .color(crate::theme::TEXT_MUTED),
    );
    ui.add_space(crate::theme::SPACE_2);
    let (core_shortcuts, layout_shortcuts) = SUPPORTED_SHORTCUTS.split_at(8);

    ui.columns(2, |columns| {
        paint_shortcut_table_section(&mut columns[0], "Core actions", core_shortcuts);
        paint_shortcut_table_section(&mut columns[1], "Layout + history", layout_shortcuts);
    });
}

fn paint_shortcut_table_section(ui: &mut egui::Ui, title: &str, shortcuts: &[SupportedShortcut]) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, crate::theme::SPACE_1);
        paint_sidebar_subsection_label(ui, title);
        ui.add_space(crate::theme::SPACE_1);
        paint_shortcut_grid(
            ui,
            egui::Id::new(("shortcut_section_grid", title)),
            shortcuts.iter().copied().map(shortcut_to_row),
        );
    });
}

fn shortcut_to_row(shortcut: SupportedShortcut) -> ShortcutRow {
    ShortcutRow {
        action: shortcut.action,
        windows: shortcut.windows,
        mac: shortcut.mac,
    }
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

pub(crate) fn show_sidebar_hint(ctx: &egui::Context, screen_rect: egui::Rect, sidebar_open: bool) {
    if sidebar_open {
        return;
    }

    egui::Area::new(egui::Id::new("sidebar_hint"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen_rect.right() - crate::theme::SPACE_4,
            screen_rect.bottom() - crate::theme::SPACE_4,
        ))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            egui::vec2(-crate::theme::SPACE_4, -crate::theme::SPACE_4),
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
                    ui.label(
                        egui::RichText::new(sidebar_shortcut_label())
                            .text_style(egui::TextStyle::Monospace)
                            .color(crate::theme::TEXT_MUTED),
                    );
                });
        });
}

fn paint_sidebar_subsection_label(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .text_style(egui::TextStyle::Small)
            .strong()
            .color(crate::theme::TEXT_SUBTLE),
    );
}

fn paint_shortcut_grid(
    ui: &mut egui::Ui,
    id: egui::Id,
    rows: impl IntoIterator<Item = ShortcutRow>,
) {
    egui::Grid::new(id)
        .num_columns(3)
        .striped(false)
        .spacing(egui::vec2(crate::theme::SPACE_1, 2.0))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Action")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("Win/Linux")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.label(
                egui::RichText::new("Mac")
                    .text_style(egui::TextStyle::Small)
                    .color(crate::theme::TEXT_SUBTLE)
                    .strong(),
            );
            ui.end_row();

            for row in rows {
                ui.label(
                    egui::RichText::new(row.action)
                        .text_style(egui::TextStyle::Small)
                        .color(crate::theme::TEXT),
                );
                paint_shortcut_keys(ui, row.windows);
                paint_shortcut_keys(ui, row.mac);
                ui.end_row();
            }
        });
}

fn paint_shortcut_keys(ui: &mut egui::Ui, shortcut: &str) {
    if shortcut == "None" {
        ui.label(
            egui::RichText::new("None")
                .text_style(egui::TextStyle::Small)
                .color(crate::theme::TEXT_SUBTLE),
        );
        return;
    }

    egui::Frame::new()
        .fill(crate::theme::SURFACE_ELEVATED)
        .stroke(crate::theme::stroke_subtle())
        .corner_radius(crate::theme::CONTROL_RADIUS.saturating_sub(2))
        .inner_margin(egui::Margin {
            left: 5,
            right: 5,
            top: 2,
            bottom: 2,
        })
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(shortcut)
                    .text_style(egui::TextStyle::Monospace)
                    .color(crate::theme::TEXT)
                    .strong(),
            );
        });
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
