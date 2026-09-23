//! The grouping artboards intentionally use a 52 px dock, 30 px controls and a 14 px radius.
//! Keep these local exceptions here; shared color and motion meanings come from ui.
use super::CanvasState;
use crate::grouping::{Board, Control, Dock, Member, Panel, State};
use crate::placement::group_pixels::{Pixels, config};
use crate::ui::{motion, theme};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Response, Sense, Stroke, Vec2};

const SEPARATOR: Color32 = Color32::from_rgb(0x2a, 0x28, 0x26);

impl CanvasState {
    pub(crate) fn current_group_dock(&self) -> Dock {
        let selected: Vec<_> = self
            .selected_placement_ids()
            .into_iter()
            .chain(self.selected_annotation_ids())
            .collect();
        let panel = if let Some(group) = self.interaction.editing_group_id {
            match selected.as_slice() {
                [member] if self.group_for_member(*member) == Some(group) => Panel::MemberDock {
                    group,
                    member: *member,
                },
                _ => Panel::Hidden,
            }
        } else if let [group] = self.interaction.selected_group_ids.as_slice() {
            if self
                .groups()
                .iter()
                .any(|g| g.id == *group && selected.iter().all(|m| g.members.contains(m)))
            {
                Panel::GroupDock {
                    group: *group,
                    align_open: false,
                }
            } else {
                Panel::Hidden
            }
        } else {
            Panel::Hidden
        };
        let mut dock = Dock {
            panel,
            text_focus: false,
        };
        let previous = &self.interaction.group_dock;
        if dock.group() == previous.group() && dock.member() == previous.member() {
            dock.text_focus = previous.text_focus;
            if let Panel::GroupDock { align_open, .. } = &mut dock.panel {
                *align_open = previous.align_shown();
            }
        }
        dock
    }
    pub(super) fn accept_group_dock(&mut self, dock: Dock) {
        let current = self.current_group_dock();
        if current.group() != dock.group() || current.member() != dock.member() {
            match dock.panel {
                Panel::GroupDock { group, .. } => self.select_group(group),
                Panel::MemberDock { group, member } => {
                    self.interaction.selected_group_ids.clear();
                    self.interaction.editing_group_id = Some(group);
                    self.interaction.selected_placement_ids =
                        if self.placement_for_id(member).is_some() {
                            vec![member]
                        } else {
                            vec![]
                        };
                    self.interaction.selected_annotation_ids =
                        if self.annotation_for_id(member).is_some() {
                            vec![member]
                        } else {
                            vec![]
                        };
                }
                Panel::Hidden => {
                    self.interaction.selected_group_ids.clear();
                    self.interaction.editing_group_id = None;
                }
            }
        }
        self.interaction.group_dock = dock;
    }
    // Only the visible group's members are needed for admission and row observations.
    // Painting the dock must not clone the entire board or run pixel layout.
    fn dock_model(&self, dock: Dock, group: &crate::data::Group) -> State<Pixels> {
        let mut board = Board::default();
        {
            board.groups.insert(group.id, config(group));
            let members: std::collections::BTreeSet<_> = group.members.iter().copied().collect();
            let geometries = self
                .document
                .placements
                .iter()
                .map(|p| (p.state.id, p.state.geometry, "Image"))
                .chain(
                    self.document
                        .annotations
                        .iter()
                        .filter_map(|a| match a.geometry {
                            crate::data::CanvasAnnotationGeometry::Rect(rect) => {
                                Some((a.id, rect, "Textbox"))
                            }
                            _ => None,
                        }),
                );
            for (m, rect, kind) in geometries.filter(|(m, _, _)| members.contains(m)) {
                let mode = group.layout.members.get(&m).copied().unwrap_or_default();
                let mut member = Member::fixed(kind, rect.visual_bounds().size().into());
                member.fill = (0..2).filter(|&a| mode.fill[a]).collect();
                member.absolute = mode.absolute;
                board.live.insert(m);
                board.members.insert(m, member);
            }
        }
        let mut state = State::init(board, Pixels::default(), self.placement_gap);
        state.dock = dock;
        state
    }
    pub(crate) fn escape_group_dock(&mut self, ctx: &egui::Context) {
        let mut state = self.grouping_state();
        state.focusText(
            state.dock.text_focus
                || ctx.egui_wants_keyboard_input()
                || self.interaction.editing_text_annotation_id.is_some(),
        );
        let was_group = state.dock.group_shown();
        state.pressKey("Escape", 0, &[], 1.0);
        if was_group && state.dock.panel == Panel::Hidden {
            self.interaction.selected_placement_ids.clear();
            self.interaction.selected_annotation_ids.clear();
        }
        self.accept_group_dock(state.dock);
    }
    pub(super) fn show_group_controls(
        &mut self,
        ui: &mut egui::Ui,
        viewport: Rect,
    ) -> Option<Rect> {
        let dock = self.current_group_dock();
        let active = dock.group().is_some();
        self.interaction.group_dock = dock.clone();
        let cache = egui::Id::new("group-dock-last");
        let current = dock
            .group()
            .and_then(|id| self.groups().iter().find(|g| g.id == id))
            .cloned();
        let progress = motion::overlay_entrance(ui.ctx(), cache, active);
        if !active && progress == 0.0 {
            return None;
        }
        // Exit paints the last presentation even when its group/items have just been deleted.
        let (mut model, group) = if let Some(group) = current {
            let model = self.dock_model(dock, &group);
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    cache,
                    (model.dock.clone(), group.clone(), model.board.clone()),
                )
            });
            (model, group)
        } else {
            let (dock, group, board) = ui
                .ctx()
                .data(|d| d.get_temp::<(Dock, crate::data::Group, Board)>(cache))?;
            let mut model = State::init(board, Pixels::default(), self.placement_gap);
            model.dock = dock;
            (model, group)
        };
        let mut field_submissions = Vec::new();
        let mut clicked_controls = Vec::new();
        let mut toggle = false;
        let mut trigger = None;
        let mut text_focus = false;
        let response = egui::Area::new(egui::Id::new("group-controls"))
            .constrain(false)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(
                viewport.center().x,
                viewport.bottom() - 24.0 + (1.0 - progress) * 52.0,
            ))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.set_max_width((viewport.width() - 48.0).max(0.0));
                ui.set_opacity(progress);
                if !active {
                    ui.disable();
                }
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                ui.spacing_mut().interact_size.y = 30.0;
                panel(14)
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 10,
                        top: 9,
                        bottom: 9,
                    })
                    .show(ui, |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("group-dock-controls")
                            .max_width((viewport.width() - 48.0 - 16.0).max(0.0))
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(0.0, 34.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        if let Some(m) = model.dock.member() {
                                            label(ui, &group.name, 13.0, theme::TEXT_MUTED);
                                            label(ui, "/", 16.0, SEPARATOR);
                                            label(
                                                ui,
                                                &format!("Member {}", model.dockMemberNumber()),
                                                13.0,
                                                theme::TEXT,
                                            );
                                            separator(ui);
                                            for (a, axis) in [(0, "W"), (1, "H")] {
                                                field_frame().show(ui, |ui| {
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(0.0, 34.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            label(
                                                                ui,
                                                                axis,
                                                                13.0,
                                                                theme::TEXT_MUTED,
                                                            );
                                                            bed(ui, |ui| {
                                                                for (fill, title) in [
                                                                    (false, "Fixed"),
                                                                    (true, "Fill"),
                                                                ] {
                                                                    let c = Control::MemberMode(
                                                                        a, fill,
                                                                    );
                                                                    let selected =
                                                                        model.board.members[&m]
                                                                            .fill
                                                                            .contains(&a)
                                                                            == fill;
                                                                    if button(
                                                                        ui,
                                                                        title,
                                                                        &format!("{axis} {title}"),
                                                                        Some(title),
                                                                        selected,
                                                                        model.controlEnabled(&c),
                                                                        false,
                                                                    )
                                                                    .clicked()
                                                                    {
                                                                        clicked_controls.push(c);
                                                                    }
                                                                }
                                                            });
                                                        },
                                                    );
                                                });
                                            }
                                            separator(ui);
                                            choose(
                                                ui,
                                                &model,
                                                Control::Absolute(
                                                    self.group_insertion_slot(
                                                        group.id,
                                                        m,
                                                        self.group_member_geometry(m)
                                                            .map_or(group.container.center, |r| {
                                                                r.center
                                                            }),
                                                    ),
                                                ),
                                                "Absolute",
                                                Some("Absolute"),
                                                model.board.members[&m].absolute,
                                                &mut clicked_controls,
                                            );
                                            separator(ui);
                                            bed(ui, |ui| {
                                                for (c, name, icon) in [
                                                    (Control::Earlier, "Move earlier", "Earlier"),
                                                    (Control::Later, "Move later", "Later"),
                                                ] {
                                                    if button(
                                                        ui,
                                                        "",
                                                        name,
                                                        Some(icon),
                                                        false,
                                                        model.controlEnabled(&c),
                                                        false,
                                                    )
                                                    .clicked()
                                                    {
                                                        clicked_controls.push(c);
                                                    }
                                                }
                                            });
                                            if button(
                                                ui,
                                                "Detach",
                                                "Detach",
                                                Some("Detach"),
                                                false,
                                                model.controlEnabled(&Control::Detach),
                                                true,
                                            )
                                            .clicked()
                                            {
                                                clicked_controls.push(Control::Detach);
                                            }
                                            ui.add_space(10.0);
                                            label(
                                                ui,
                                                "esc leaves member editing",
                                                11.0,
                                                theme::TEXT_SUBTLE,
                                            );
                                        } else {
                                            let (name, focused) = field(
                                                ui,
                                                "Group name",
                                                &group.name,
                                                116.0,
                                                true,
                                                14.0,
                                            );
                                            text_focus |= focused;
                                            if let Some(name) = name {
                                                field_submissions.push(Control::Name(name));
                                            }
                                            ui.add_space(4.0);
                                            label(
                                                ui,
                                                &format!(
                                                    "{} {}",
                                                    model.dockCount(),
                                                    if model.dockCount() == 1 {
                                                        "item"
                                                    } else {
                                                        "items"
                                                    }
                                                ),
                                                12.0,
                                                theme::TEXT_SUBTLE,
                                            );
                                            separator(ui);
                                            bed(ui, |ui| {
                                                for flow in ["Row", "Column", "Wrap", "Free"] {
                                                    let c = Control::Flow(flow.into());
                                                    if button(
                                                        ui,
                                                        flow,
                                                        flow,
                                                        Some(flow),
                                                        group.flow.as_str() == flow,
                                                        model.controlEnabled(&c),
                                                        false,
                                                    )
                                                    .clicked()
                                                    {
                                                        clicked_controls.push(c);
                                                    }
                                                }
                                            });
                                            separator(ui);
                                            for (name, number, c) in [
                                                ("Gap", group.gap, Control::Gap(group.gap)),
                                                (
                                                    "Row gap",
                                                    group.layout.row_gap,
                                                    Control::RowGap(group.layout.row_gap),
                                                ),
                                                (
                                                    "Padding",
                                                    group.padding,
                                                    Control::Padding(group.padding),
                                                ),
                                            ] {
                                                if !model.controlShown(&c) {
                                                    continue;
                                                }
                                                field_frame().show(ui, |ui| {
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(0.0, 30.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            icon_space(ui, name, theme::TEXT_MUTED);
                                                            let (value, focused) = field(
                                                                ui,
                                                                name,
                                                                &format!("{number:.0}"),
                                                                34.0,
                                                                model.controlEnabled(&c),
                                                                13.0,
                                                            );
                                                            text_focus |= focused;
                                                            if let Some(n) = value
                                                                .and_then(|v| match c {
                                                                    Control::Gap(_)
                                                                    | Control::RowGap(_) => v
                                                                        .parse::<i64>()
                                                                        .ok()
                                                                        .map(|n| n as f32),
                                                                    _ => v.parse::<f32>().ok(),
                                                                })
                                                                .filter(|n| n.is_finite())
                                                            {
                                                                field_submissions.push(match c {
                                                                    Control::Gap(_) => {
                                                                        Control::Gap(n)
                                                                    }
                                                                    Control::RowGap(_) => {
                                                                        Control::RowGap(n)
                                                                    }
                                                                    _ => Control::Padding(n),
                                                                });
                                                            }
                                                        },
                                                    );
                                                });
                                            }
                                            separator(ui);
                                            let align = button(
                                                ui,
                                                "",
                                                "Alignment",
                                                Some("Alignment"),
                                                false,
                                                model.alignEnabled(),
                                                false,
                                            );
                                            preview(
                                                ui.painter(),
                                                Rect::from_min_size(
                                                    align.rect.left_center()
                                                        + egui::vec2(10.0, -13.5),
                                                    egui::vec2(27.0, 27.0),
                                                ),
                                                group.layout.align,
                                            );
                                            toggle = align.clicked();
                                            trigger = Some(align.rect);
                                            separator(ui);
                                            for (a, axis) in [(0, "W"), (1, "H")] {
                                                field_frame().show(ui, |ui| {
                                                    ui.allocate_ui_with_layout(
                                                        egui::vec2(0.0, 30.0),
                                                        egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            label(
                                                                ui,
                                                                axis,
                                                                13.0,
                                                                theme::TEXT_MUTED,
                                                            );
                                                            let value = if group.layout.fixed[a] {
                                                                format!(
                                                                    "{:.0}",
                                                                    group.container.scaled_size()
                                                                        [a]
                                                                )
                                                            } else {
                                                                "Hug".into()
                                                            };
                                                            let (value, focused) = field(
                                                                ui,
                                                                &format!("{axis} size"),
                                                                &value,
                                                                30.0,
                                                                true,
                                                                13.0,
                                                            );
                                                            text_focus |= focused;
                                                            if let Some(n) = value
                                                                .and_then(|v| v.parse::<f32>().ok())
                                                                .filter(|n| {
                                                                    n.is_finite() && *n > 0.0
                                                                })
                                                            {
                                                                field_submissions
                                                                    .push(Control::Dimension(a, n));
                                                            }
                                                            let c = Control::Hug(a);
                                                            if small_hug(
                                                                ui,
                                                                axis,
                                                                !group.layout.fixed[a],
                                                                model.controlEnabled(&c),
                                                            )
                                                            .clicked()
                                                            {
                                                                clicked_controls.push(c);
                                                            }
                                                        },
                                                    );
                                                });
                                            }
                                            separator(ui);
                                            choose(
                                                ui,
                                                &model,
                                                Control::Clip,
                                                "Clip",
                                                Some("Clip"),
                                                group.layout.clip,
                                                &mut clicked_controls,
                                            );
                                            separator(ui);
                                            if button(
                                                ui,
                                                "Ungroup",
                                                "Ungroup",
                                                Some("Ungroup"),
                                                false,
                                                model.controlEnabled(&Control::Ungroup),
                                                true,
                                            )
                                            .clicked()
                                            {
                                                clicked_controls.push(Control::Ungroup);
                                            }
                                        }
                                    },
                                );
                            });
                    });
            });
        let mut bounds = response.response.rect;
        let mut dismiss = false;
        if model.dock.align_shown()
            && let Some(anchor) = trigger
        {
            let popup = egui::Area::new(egui::Id::new("group-alignment-popover"))
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(egui::pos2(
                    anchor.right() + 26.0,
                    response.response.rect.top() - 8.0,
                ))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    panel(12).inner_margin(10).show(ui, |ui| {
                        label(ui, "ALIGN", 10.0, theme::TEXT_SUBTLE);
                        ui.add_space(4.0);
                        for y in 0..3 {
                            ui.horizontal(|ui| {
                                for x in 0..3 {
                                    let c = Control::Align([x, y]);
                                    let name = format!(
                                        "Align {}, {}",
                                        ["left", "center", "right"][x],
                                        ["top", "center", "bottom"][y]
                                    );
                                    let selected = group.layout.align == [x, y];
                                    let r = cell(
                                        ui,
                                        &name,
                                        egui::vec2(28.0, 28.0),
                                        selected,
                                        model.controlEnabled(&c),
                                        theme::SURFACE_ELEVATED,
                                    );
                                    ui.painter().rect_filled(
                                        Rect::from_center_size(r.rect.center(), Vec2::splat(6.0)),
                                        2,
                                        if selected {
                                            theme::BACKGROUND
                                        } else {
                                            theme::TEXT_SUBTLE
                                        },
                                    );
                                    if r.clicked() {
                                        clicked_controls.push(c);
                                    }
                                }
                            });
                        }
                        ui.add_space(8.0);
                        label(ui, "AUTO SPACING", 10.0, theme::TEXT_SUBTLE);
                        ui.add_space(4.0);
                        bed(ui, |ui| {
                            for (mode, title) in [
                                ("Number", "Off"),
                                ("Between", "Between"),
                                ("Evenly", "Evenly"),
                                ("Around", "Around"),
                            ] {
                                let c = Control::Auto(mode.into());
                                if button(
                                    ui,
                                    title,
                                    title,
                                    None,
                                    group.layout.auto == mode,
                                    model.controlEnabled(&c),
                                    false,
                                )
                                .clicked()
                                {
                                    clicked_controls.push(c);
                                }
                            }
                        });
                    });
                });
            // Return the overlay under the pointer; a bounding union would swallow canvas clicks beside the popover.
            if ui.input(|i| {
                i.pointer
                    .latest_pos()
                    .is_some_and(|p| popup.response.rect.contains(p))
            }) {
                bounds = popup.response.rect;
            }
            dismiss = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !popup.response.rect.contains(p) && !anchor.contains(p))
            });
        }
        // Focus/dismissal never run a layout pass. Only accepted control edits enter the pixel adapter.
        if dismiss {
            model.dismissAlign();
        }
        if toggle {
            model.toggleAlign();
        }
        if model.dock.text_focus != text_focus {
            model.focusText(text_focus);
        }
        if active {
            self.interaction.group_dock = model.dock;
        }
        // Blur commits belong to the old control context, even when the clicked
        // command was rendered earlier and will change which fields are enabled.
        for control in field_submissions.into_iter().chain(clicked_controls) {
            let _ = self.group_edit(|s| s.useDock(control));
        }
        Some(bounds)
    }
}

fn panel(radius: u8) -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(radius)
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(180),
        })
}
fn field_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE_ELEVATED)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 0))
}
fn bed(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE_PRESSED)
        .corner_radius(10)
        .inner_margin(2)
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(0.0, 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    contents(ui);
                },
            );
        });
}
fn label(ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
    ui.label(egui::RichText::new(text).size(size).color(color));
}
fn separator(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 20.0), Sense::hover());
    ui.painter().line_segment(
        [r.center_top(), r.center_bottom()],
        Stroke::new(1.0_f32, SEPARATOR),
    );
}
fn cell(
    ui: &mut egui::Ui,
    name: &str,
    size: Vec2,
    selected: bool,
    enabled: bool,
    fill: Color32,
) -> Response {
    ui.add_enabled_ui(enabled, |ui| {
        let (r, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, name)
        });
        let color = if selected {
            theme::ACCENT
        } else if response.is_pointer_button_down_on() {
            theme::SURFACE_PRESSED
        } else if response.hovered() {
            theme::SURFACE_ELEVATED
        } else {
            fill
        };
        ui.painter().rect_filled(r, 8, color);
        if response.has_focus() {
            ui.painter().rect_stroke(
                r,
                8,
                Stroke::new(1.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        response
    })
    .inner
}
fn button(
    ui: &mut egui::Ui,
    text: &str,
    name: &str,
    icon: Option<&str>,
    selected: bool,
    enabled: bool,
    muted: bool,
) -> Response {
    let text_width = ui
        .painter()
        .layout_no_wrap(text.into(), FontId::proportional(13.0), theme::TEXT)
        .size()
        .x;
    let icon_width = if icon == Some("Alignment") {
        47.0
    } else if icon.is_some() {
        14.0
    } else {
        0.0
    };
    let width = 20.0
        + text_width
        + icon_width
        + if icon.is_some() && !text.is_empty() {
            6.0
        } else {
            0.0
        };
    let r = cell(
        ui,
        name,
        egui::vec2(width, 30.0),
        selected,
        enabled,
        if muted {
            Color32::TRANSPARENT
        } else {
            theme::SURFACE_PRESSED
        },
    );
    let color = if selected {
        theme::BACKGROUND
    } else if muted {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    };
    let color = if enabled {
        color
    } else {
        color.gamma_multiply(0.4)
    };
    if let Some(icon) = icon {
        paint_icon(
            ui.painter(),
            r.rect.left_center() + egui::vec2(10.0 + icon_width * 0.5, 0.0),
            icon,
            color,
        );
    }
    if !text.is_empty() {
        ui.painter().text(
            r.rect.right_center() - egui::vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            text,
            FontId::proportional(13.0),
            color,
        );
    }
    r.on_hover_text(name)
}
fn choose(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    c: Control,
    text: &str,
    icon: Option<&str>,
    selected: bool,
    clicked_controls: &mut Vec<Control>,
) {
    if button(
        ui,
        text,
        text,
        icon,
        selected,
        model.controlEnabled(&c),
        false,
    )
    .clicked()
    {
        clicked_controls.push(c);
    }
}
fn small_hug(ui: &mut egui::Ui, axis: &str, selected: bool, enabled: bool) -> Response {
    let r = cell(
        ui,
        &format!("{axis} hug contents"),
        Vec2::splat(22.0),
        selected,
        enabled,
        SEPARATOR,
    );
    paint_icon(
        ui.painter(),
        r.rect.center(),
        "Hug",
        if selected {
            theme::BACKGROUND
        } else {
            theme::TEXT_MUTED
        }
        .gamma_multiply(if enabled { 1.0 } else { 0.4 }),
    );
    r
}
fn field(
    ui: &mut egui::Ui,
    name: &str,
    value: &str,
    width: f32,
    enabled: bool,
    size: f32,
) -> (Option<String>, bool) {
    let id = ui.make_persistent_id(name);
    let key = id.with("draft");
    let mut draft = ui
        .ctx()
        .data(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| value.into());
    let was_focused = ui.memory(|m| m.has_focus(id));
    let output = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                egui::vec2(width, 30.0),
                egui::TextEdit::singleline(&mut draft)
                    .id(id)
                    .desired_width(width)
                    .font(FontId::proportional(size))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::symmetric(
                        if name == "Group name" { 10 } else { 0 },
                        0,
                    ))
                    .vertical_align(egui::Align::Center),
            )
        })
        .inner;
    output.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, name));
    let escape =
        (was_focused || output.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = output.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let outside = output.has_focus()
        && ui.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !output.rect.contains(p))
        });
    let submit = output.lost_focus() || enter || outside;
    if escape {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    }
    if escape || enter || outside {
        output.surrender_focus();
    }
    if escape || submit || !output.has_focus() {
        ui.ctx().data_mut(|d| d.remove::<String>(key));
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(key, draft.clone()));
    }
    (
        (submit && !escape && draft != value).then_some(draft),
        output.has_focus() && !escape && !enter,
    )
}
fn preview(p: &egui::Painter, rect: Rect, align: [usize; 2]) {
    for y in 0..3 {
        for x in 0..3 {
            p.rect_filled(
                Rect::from_min_size(
                    rect.min + egui::vec2(x as f32 * 10.0, y as f32 * 10.0),
                    Vec2::splat(7.0),
                ),
                2,
                if align == [x, y] {
                    theme::ACCENT
                } else {
                    Color32::from_rgb(74, 71, 68)
                },
            );
        }
    }
}
fn icon_space(ui: &mut egui::Ui, name: &str, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    paint_icon(ui.painter(), r.center(), name, color);
}
fn paint_icon(p: &egui::Painter, center: egui::Pos2, name: &str, color: Color32) {
    let origin = center - Vec2::splat(7.0);
    let point = |x: f32, y: f32| origin + egui::vec2(x, y) * (14.0 / 16.0);
    let line = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            Stroke::new(1.25_f32, color),
        ));
    };
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            0,
            Stroke::new(1.25_f32, color),
            egui::StrokeKind::Inside,
        );
    };
    let dashed = |points: &[(f32, f32)]| {
        p.extend(egui::Shape::dashed_line(
            &points.iter().map(|&(x, y)| point(x, y)).collect::<Vec<_>>(),
            Stroke::new(1.25_f32, color),
            1.75,
            1.3,
        ));
    };
    match name {
        "Row" => {
            for x in [1.5, 6.0, 10.5] {
                rect(x, 4.0, 4.0, 8.0);
            }
        }
        "Column" => {
            for y in [1.5, 6.0, 10.5] {
                rect(4.0, y, 8.0, 4.0);
            }
        }
        "Wrap" => {
            for (x, y) in [(1.5, 1.5), (9.0, 1.5), (1.5, 9.0)] {
                rect(x, y, 5.5, 5.5);
            }
        }
        "Free" => {
            rect(1.5, 2.0, 6.0, 5.0);
            rect(8.0, 7.5, 6.0, 6.0);
        }
        "Ungroup" => {
            rect(1.5, 1.5, 7.0, 7.0);
            dashed(&[
                (7.5, 7.5),
                (14.5, 7.5),
                (14.5, 14.5),
                (7.5, 14.5),
                (7.5, 7.5),
            ]);
        }
        "Gap" => {
            line(&[(2., 8.), (14., 8.)]);
            line(&[(5., 5.), (2., 8.), (5., 11.)]);
            line(&[(11., 5.), (14., 8.), (11., 11.)]);
        }
        "Row gap" => {
            line(&[(8., 2.), (8., 14.)]);
            line(&[(5., 5.), (8., 2.), (11., 5.)]);
            line(&[(5., 11.), (8., 14.), (11., 11.)]);
        }
        "Padding" => {
            rect(1.5, 1.5, 13., 13.);
            dashed(&[(5., 5.), (11., 5.), (11., 11.), (5., 11.), (5., 5.)]);
        }
        "Clip" => {
            rect(2., 2., 12., 12.);
            dashed(&[(9., 6.), (14., 6.)]);
            dashed(&[(9., 10.), (14., 10.)]);
        }
        "Absolute" => {
            dashed(&[(2., 2.), (14., 2.), (14., 14.), (2., 14.), (2., 2.)]);
            p.circle_filled(point(10., 6.), 1.75, color);
        }
        "Earlier" => line(&[(10., 3.), (5., 8.), (10., 13.)]),
        "Later" => line(&[(6., 3.), (11., 8.), (6., 13.)]),
        "Detach" => {
            line(&[(4., 4.), (12., 12.)]);
            line(&[(12., 4.), (4., 12.)]);
        }
        "Hug" => {
            line(&[(2., 8.), (5., 8.)]);
            line(&[(11., 8.), (14., 8.)]);
            rect(6., 4., 4., 8.);
        }
        "Fixed" | "Fill" => {
            line(&[(2., 3.), (2., 13.)]);
            line(&[(14., 3.), (14., 13.)]);
            line(&[(5., 8.), (11., 8.)]);
            if name == "Fill" {
                line(&[(6., 6.), (4., 8.), (6., 10.)]);
                line(&[(10., 6.), (12., 8.), (10., 10.)]);
            }
        }
        "Alignment" => {
            let c = center + egui::vec2(17.0, 0.0);
            p.line_segment(
                [c - egui::vec2(3.0, 1.5), c + egui::vec2(0.0, 1.5)],
                Stroke::new(1.25_f32, color),
            );
            p.line_segment(
                [c + egui::vec2(0.0, 1.5), c + egui::vec2(3.0, -1.5)],
                Stroke::new(1.25_f32, color),
            );
        }
        _ => {}
    }
}
