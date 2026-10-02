//! The Group card sets the shared card vocabulary in `ui::card`. Its width,
//! align pad, and icon set are its own and stay here.
use super::CanvasState;
use crate::data::{GapMode, GroupFlow};
use crate::grouping::{Board, Control, Dock, Member, MemberKind, Panel, State};
use crate::placement::group_pixels::{Pixels, config};
use crate::ui::card::{self, bed, cell, ink, tip};
use crate::ui::{motion, shortcut, theme};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Response, Sense, Stroke, Vec2};

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
                Panel::GroupDock { group: *group }
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
        }
        dock
    }
    /// A scrub edits the Group every frame; history takes it as one edit on release.
    pub(crate) fn group_dock_scrubbing(&self) -> bool {
        self.interaction.group_dock_scrubbing
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
                .map(|p| (p.state.id, p.state.geometry, MemberKind::Image))
                .chain(
                    self.document
                        .annotations
                        .iter()
                        .filter_map(|a| match a.geometry {
                            crate::data::CanvasAnnotationGeometry::Rect(rect) => {
                                Some((a.id, rect, MemberKind::Textbox))
                            }
                            _ => None,
                        }),
                );
            for (m, rect, kind) in geometries.filter(|(m, _, _)| members.contains(m)) {
                let mode = group.layout.members.get(&m).copied().unwrap_or_default();
                let mut member = Member::fixed(kind, rect.visual_bounds().size().into());
                member.fill = mode.fill;
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
        state.focus_text(
            state.dock.text_focus
                || ctx.egui_wants_keyboard_input()
                || self.interaction.editing_text_annotation_id.is_some(),
        );
        let was_group = state.dock.group_shown();
        state.escape();
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
        self.interaction.group_dock_scrubbing = false;
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
        let mut input = DockInput::default();
        let absolute = model.dock.member().map(|m| {
            Control::Absolute(
                self.group_insertion_slot(
                    group.id,
                    m,
                    self.group_member_geometry(m)
                        .map_or(group.container.center, |r| r.center),
                ),
            )
        });
        // A fixed spot keeps the card still while layout edits move the Group.
        let pos = egui::pos2(
            viewport.right() - 16.0 + (1.0 - progress) * CARD_SLIDE,
            viewport.center().y,
        );
        // Short windows scroll the card instead of pushing controls offscreen.
        let max_height = viewport.height() - 32.0 - 2.0 * card::PADDING;
        let bounds = overlay(ui.ctx(), pos, progress, active, |ui| {
            card::frame().show(ui, |ui| {
                ui.set_width(CARD_INNER);
                // An Area offers last frame's size, which would never let the card grow back.
                ui.set_max_height(max_height);
                egui::ScrollArea::vertical()
                    .id_salt("group-card-scroll")
                    .show(ui, |ui| {
                        ui.set_width(CARD_INNER);
                        ui.spacing_mut().item_spacing = card::ITEM_SPACING;
                        match &absolute {
                            Some(absolute) => member_card(ui, &model, &group, absolute, &mut input),
                            None => group_card(ui, &model, &group, &mut input),
                        }
                    });
            });
        });
        let DockInput {
            field_submissions,
            clicked_controls,
            text_focus,
            scrubbing,
            back,
        } = input;
        self.interaction.group_dock_scrubbing = scrubbing;
        // Focus never runs a layout pass. Only accepted control edits enter the pixel adapter.
        if model.dock.text_focus != text_focus {
            model.focus_text(text_focus);
        }
        if active {
            self.interaction.group_dock = model.dock;
        }
        // Blur commits belong to the old control context, even when the clicked
        // command was rendered earlier and will change which fields are enabled.
        for control in field_submissions.into_iter().chain(clicked_controls) {
            let _ = self.group_edit(|s| s.use_dock(control));
        }
        if back {
            self.accept_group_dock(Dock::for_group(group.id));
        }
        Some(bounds)
    }
}

const CARD: &str = "group-card";
const CARD_WIDTH: f32 = 264.0;
const CARD_INNER: f32 = CARD_WIDTH - 2.0 * card::PADDING;
/// Two half-width fields on one card line, 6 pixels apart.
const CARD_HALF: f32 = (CARD_INNER - 6.0) * 0.5;
const CARD_SLIDE: f32 = 8.0;
const PAD_SIZE: f32 = 70.0;
const DOT: Color32 = Color32::from_rgb(74, 71, 68);

#[derive(Default)]
struct DockInput {
    field_submissions: Vec<Control>,
    clicked_controls: Vec<Control>,
    text_focus: bool,
    scrubbing: bool,
    /// The member card's group name returns to the Group card.
    back: bool,
}

fn overlay(
    ctx: &egui::Context,
    pos: egui::Pos2,
    progress: f32,
    active: bool,
    contents: impl FnOnce(&mut egui::Ui),
) -> Rect {
    egui::Area::new(egui::Id::new(CARD))
        .constrain(false)
        .pivot(Align2::RIGHT_CENTER)
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_opacity(progress);
            if !active {
                ui.disable();
            }
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.spacing_mut().interact_size.y = theme::CONTROL_HEIGHT;
            contents(ui);
        })
        .response
        .rect
}

fn group_card(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    input: &mut DockInput,
) {
    let count = model.dock_count();
    let count = format!("{count} {}", if count == 1 { "item" } else { "items" });
    let ungroup = model.control_enabled(&Control::Ungroup);
    let ((name, focused), r) = card_header(ui, &count, "Ungroup", ungroup, |ui, width| {
        card::field(
            ui,
            "Group name",
            &group.name,
            width,
            22.0,
            true,
            theme::TYPE_TITLE,
            egui::Align::Min,
        )
    });
    input.text_focus |= focused;
    if let Some(name) = name {
        input.field_submissions.push(Control::Name(name));
    }
    if tip(r, &shortcut::text(crate::app::UNGROUP)).clicked() {
        input.clicked_controls.push(Control::Ungroup);
    }
    card::section(ui, "Layout");
    bed(ui, |ui| {
        for flow in [
            GroupFlow::Row,
            GroupFlow::Column,
            GroupFlow::Wrap,
            GroupFlow::Free,
        ] {
            let c = Control::Flow(flow);
            let name = flow.as_str();
            let selected = group.flow == flow;
            let enabled = model.control_enabled(&c);
            let r = cell(
                ui,
                name,
                egui::vec2((CARD_INNER - 10.0) / 4.0, theme::CONTROL_HEIGHT),
                selected,
                enabled,
                theme::SURFACE_PRESSED,
            );
            paint_icon(
                ui.painter(),
                r.rect.center(),
                name,
                ink(selected, enabled, false),
                18.0,
            );
            if tip(r, &name.to_lowercase()).clicked() {
                input.clicked_controls.push(c);
            }
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        align_pad(ui, model, group, input);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            spacing_line(ui, model, group, input);
            spread_bed(ui, model, group, input);
        });
    });
    card::section(ui, "Frame");
    ui.horizontal(|ui| {
        size_field(ui, model, group, input, 0);
        size_field(ui, model, group, input, 1);
    });
    ui.horizontal(|ui| {
        number_field(
            ui,
            model,
            input,
            Control::Padding(group.padding),
            ("Padding", "Padding"),
            CARD_HALF,
        );
        let enabled = model.control_enabled(&Control::Clip);
        if tip(
            dock_switch(ui, "Clip", CARD_HALF, group.layout.clip, enabled),
            "hide overflow",
        )
        .clicked()
        {
            input.clicked_controls.push(Control::Clip);
        }
    });
}

/// A gold rule ties the card to the gold outline on the canvas. The title
/// leads, the subtitle sits below, and a text action closes the line.
fn card_header<R>(
    ui: &mut egui::Ui,
    subtitle: &str,
    action: &str,
    action_enabled: bool,
    title: impl FnOnce(&mut egui::Ui, f32) -> R,
) -> (R, Response) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 9.0;
        let (rule, _) = ui.allocate_exact_size(egui::vec2(3.0, 34.0), Sense::hover());
        ui.painter().rect_filled(rule, 1.5, theme::ACCENT);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = card::action(ui, action, action_enabled);
            let title_width = ui.available_width();
            let subtitle_height =
                ui.fonts_mut(|f| f.row_height(&FontId::proportional(theme::TYPE_DETAIL)));
            let title = ui
                .allocate_ui_with_layout(
                    egui::vec2(title_width, 22.0 + subtitle_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let title = title(ui, title_width);
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(theme::TYPE_DETAIL)
                                .color(theme::TEXT_MUTED),
                        );
                        title
                    },
                )
                .inner;
            (title, r)
        })
        .inner
    })
    .inner
}

/// Member editing swaps the card's contents in place. The Group name leads
/// back to the Group card; resizing stays on the canvas corner handles and
/// reordering on drag and the arrow keys.
fn member_card(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    absolute: &Control,
    input: &mut DockInput,
) {
    let Some(m) = model.dock.member() else {
        return;
    };
    let member = &model.board.members[&m];
    let subtitle = format!(
        "Member {} of {}",
        model.dock_member_number(),
        model.dock_count()
    );
    let detach = model.control_enabled(&Control::Detach);
    let (back, r) = card_header(ui, &subtitle, "Detach", detach, |ui, width| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 22.0), Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &group.name));
        let color = if response.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        };
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            format!("‹ {}", group.name),
            FontId::proportional(theme::TYPE_TITLE),
            color,
        );
        tip(response, "back to group").clicked()
    });
    input.back |= back;
    if tip(r, "leave the group").clicked() {
        input.clicked_controls.push(Control::Detach);
    }
    card::section(ui, "Frame");
    bed(ui, |ui| {
        for (a, name) in [(0, "Fill width"), (1, "Fill height")] {
            let c = Control::MemberMode(a, !member.fill[a]);
            let enabled = model.control_enabled(&c);
            let r = cell(
                ui,
                name,
                egui::vec2((CARD_INNER - 6.0) / 2.0, theme::CONTROL_HEIGHT),
                member.fill[a],
                enabled,
                theme::SURFACE_PRESSED,
            );
            let color = ink(member.fill[a], enabled, false);
            paint_icon(
                ui.painter(),
                r.rect.left_center() + egui::vec2(20.0, 0.0),
                name,
                color,
                14.0,
            );
            ui.painter().text(
                r.rect.left_center() + egui::vec2(34.0, 0.0),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(theme::TYPE_CONTROL),
                color,
            );
            if tip(r, &name.to_lowercase()).clicked() {
                input.clicked_controls.push(c);
            }
        }
    });
    let enabled = model.control_enabled(absolute);
    if tip(
        dock_switch(ui, "Absolute", CARD_INNER, member.absolute, enabled),
        "place freely, outside the flow",
    )
    .clicked()
    {
        input.clicked_controls.push(absolute.clone());
    }
}

/// The 3×3 alignment pad. Its cells sit on a fixed grid and every mark is
/// painted inside its cell, so the larger selected glyph never moves the dots.
/// When a spread mode owns the main axis, the pad picks one of three
/// cross-axis bands over the same grid instead.
fn align_pad(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    input: &mut DockInput,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(PAD_SIZE), Sense::hover());
    ui.painter().rect_filled(rect, 10, theme::SURFACE_PRESSED);
    let grid = rect.shrink(4.0);
    let step = grid.width() / 3.0;
    let cell_at = |x: usize, y: usize| {
        Rect::from_min_size(
            grid.min + egui::vec2(x as f32, y as f32) * step,
            Vec2::splat(step),
        )
    };
    let column = group.flow == GroupFlow::Column;
    let cross = if column { 0 } else { 1 };
    let align = group.layout.align;
    let spread = group.layout.auto != GapMode::Number;
    let glyphs = if column {
        ["Align left", "Align center", "Align right"]
    } else {
        ["Align top", "Align middle", "Align bottom"]
    };
    let targets: Vec<([usize; 2], Vec<Rect>, String, String)> = if spread {
        (0..3)
            .map(|c| {
                let mut xy = align;
                xy[cross] = c;
                let cells = (0..3)
                    .map(|i| if column { cell_at(c, i) } else { cell_at(i, c) })
                    .collect();
                let tooltip = glyphs[c].trim_start_matches("Align ").to_lowercase();
                (xy, cells, glyphs[c].to_string(), tooltip)
            })
            .collect()
    } else {
        (0..9)
            .map(|i| {
                let xy = [i % 3, i / 3];
                let name = format!(
                    "Align {}, {}",
                    ["left", "center", "right"][xy[0]],
                    ["top", "center", "bottom"][xy[1]]
                );
                let tooltip = [
                    ["top left", "top", "top right"],
                    ["left", "center", "right"],
                    ["bottom left", "bottom", "bottom right"],
                ][xy[1]][xy[0]];
                (xy, vec![cell_at(xy[0], xy[1])], name, tooltip.to_string())
            })
            .collect()
    };
    for (xy, cells, name, tooltip) in targets {
        let c = Control::Align(xy);
        let enabled = model.control_enabled(&c);
        let selected = xy == align;
        let target = cells[0].union(cells[cells.len() - 1]);
        // Interacting in place, without a child scope, keeps the cells from
        // taking layout space.
        let response = ui.interact(
            target,
            egui::Id::new(("group-card-align", xy, spread)),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, &name)
        });
        let response = tip(response, &tooltip);
        let p = ui.painter();
        if (selected && spread) || response.hovered() {
            p.rect_filled(target.shrink(1.0), 6, theme::SURFACE_ELEVATED);
        }
        if response.has_focus() {
            p.rect_stroke(
                target.shrink(1.0),
                6,
                Stroke::new(1.0_f32, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let fade = if enabled { 1.0 } else { 0.4 };
        if selected {
            paint_icon(
                p,
                target.center(),
                glyphs[xy[cross]],
                theme::ACCENT.gamma_multiply(fade),
                16.0,
            );
        } else {
            for cell in cells {
                p.rect_filled(
                    Rect::from_center_size(cell.center(), Vec2::splat(4.0)),
                    1,
                    DOT.gamma_multiply(fade),
                );
            }
        }
        if response.clicked() {
            input.clicked_controls.push(c);
        }
    }
}

/// Gap beside the pad; Wrap adds row gap on the same line. The gap glyph
/// follows the main axis.
fn spacing_line(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    input: &mut DockInput,
) {
    let width = CARD_INNER - PAD_SIZE - 8.0;
    let gap_icon = if group.flow == GroupFlow::Column {
        "Row gap"
    } else {
        "Gap"
    };
    let gap = Control::Gap(group.gap);
    let row_gap = Control::RowGap(group.layout.row_gap);
    if model.control_shown(&row_gap) {
        ui.horizontal(|ui| {
            let half = (width - 6.0) * 0.5;
            number_field(ui, model, input, gap, ("Gap", gap_icon), half);
            number_field(ui, model, input, row_gap, ("Row gap", "Row gap"), half);
        });
    } else {
        number_field(ui, model, input, gap, ("Gap", gap_icon), width);
    }
}

fn spread_bed(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    input: &mut DockInput,
) {
    let main = usize::from(group.flow == GroupFlow::Column);
    let packed = [[2.0, 5.0, 8.0], [4.0, 7.0, 10.0], [6.0, 9.0, 12.0]][group.layout.align[main]];
    bed(ui, |ui| {
        for (mode, name, tooltip, bars) in [
            (GapMode::Number, "Off", "packed", packed),
            (
                GapMode::Between,
                "Between",
                "space between",
                [2.0, 7.0, 12.0],
            ),
            (GapMode::Evenly, "Evenly", "space evenly", [3.5, 7.0, 10.5]),
            (GapMode::Around, "Around", "space around", [3.0, 7.0, 11.0]),
        ] {
            let c = Control::Auto(mode);
            let selected = group.layout.auto == mode;
            let enabled = model.control_enabled(&c);
            let r = cell(
                ui,
                name,
                egui::vec2(38.0, theme::CONTROL_HEIGHT),
                selected,
                enabled,
                theme::SURFACE_PRESSED,
            );
            paint_spread(
                ui.painter(),
                r.rect.center(),
                bars,
                main == 1,
                ink(selected, enabled, false),
            );
            if tip(r, tooltip).clicked() {
                input.clicked_controls.push(c);
            }
        }
    });
}

/// Paints a main-axis distribution glyph on a 16 unit grid: two faint edge
/// rules and three bars at `bars`, transposed for columns.
fn paint_spread(
    p: &egui::Painter,
    center: egui::Pos2,
    bars: [f32; 3],
    vertical: bool,
    color: Color32,
) {
    let origin = center - Vec2::splat(8.0);
    let point = |a: f32, b: f32| {
        origin
            + if vertical {
                egui::vec2(b, a)
            } else {
                egui::vec2(a, b)
            }
    };
    let rule = Stroke::new(1.3_f32, color.gamma_multiply(0.45));
    for a in [0.75, 15.25] {
        p.line_segment([point(a, 3.0), point(a, 13.0)], rule);
    }
    for a in bars {
        p.rect_filled(
            Rect::from_two_pos(point(a, 4.0), point(a + 2.0, 12.0)),
            0.5,
            color,
        );
    }
}

fn number_field(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    input: &mut DockInput,
    c: Control,
    (name, icon): (&str, &str),
    width: f32,
) {
    let (Control::Gap(number) | Control::RowGap(number) | Control::Padding(number)) = c else {
        unreachable!()
    };
    let enabled = model.control_enabled(&c);
    let with = |n| match c {
        Control::Gap(_) => Control::Gap(n),
        Control::RowGap(_) => Control::RowGap(n),
        _ => Control::Padding(n),
    };
    card::well(ui, width, |ui| {
        let tooltip = name.to_lowercase();
        let (scrubbed, dragging) =
            card::scrub(ui, (name, &tooltip), number, 0.0, enabled, |p, r, color| {
                paint_icon(p, r.center(), icon, color, 14.0)
            });
        input.scrubbing |= dragging;
        if let Some(n) = scrubbed {
            input.field_submissions.push(with(n));
        }
        let width = ui.available_width();
        let (value, focused) = card::field(
            ui,
            name,
            &format!("{number:.0}"),
            width,
            theme::CONTROL_HEIGHT,
            enabled,
            theme::TYPE_CONTROL,
            egui::Align::Min,
        );
        input.text_focus |= focused;
        if let Some(n) = value
            .and_then(|v| match c {
                Control::Gap(_) | Control::RowGap(_) => v.parse::<i64>().ok().map(|n| n as f32),
                _ => v.parse::<f32>().ok(),
            })
            .filter(|n| n.is_finite())
        {
            input.field_submissions.push(with(n));
        }
    });
}

/// Shows the outer size even while the axis hugs, dimmed, so the number and
/// the mode are both visible. Submitting a changed size fixes the axis.
fn size_field(
    ui: &mut egui::Ui,
    model: &State<Pixels>,
    group: &crate::data::Group,
    input: &mut DockInput,
    a: usize,
) {
    let axis = ["W", "H"][a];
    let hugging = !group.layout.fixed[a];
    let size = group.container.scaled_size()[a];
    card::well(ui, CARD_HALF, |ui| {
        let enabled = model.control_enabled(&Control::Dimension(a, size));
        let name = (axis, ["width", "height"][a]);
        let (scrubbed, dragging) =
            card::scrub(ui, name, size.round(), 1.0, enabled, |p, r, color| {
                p.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    axis,
                    FontId::proportional(theme::TYPE_CONTROL),
                    color,
                );
            });
        input.scrubbing |= dragging;
        if let Some(n) = scrubbed {
            input.field_submissions.push(Control::Dimension(a, n));
        }
        let chip_font = FontId::proportional(theme::TYPE_CHIP);
        let chip_width = ui
            .painter()
            .layout_no_wrap("Hug".into(), chip_font.clone(), theme::TEXT)
            .size()
            .x
            + 14.0;
        let width = ui.available_width() - chip_width - ui.spacing().item_spacing.x;
        if hugging {
            ui.visuals_mut().override_text_color = Some(theme::TEXT_MUTED);
        }
        let (value, focused) = card::field(
            ui,
            &format!("{axis} size"),
            &format!("{size:.0}"),
            width,
            theme::CONTROL_HEIGHT,
            true,
            theme::TYPE_CONTROL,
            egui::Align::Min,
        );
        input.text_focus |= focused;
        if let Some(n) = value
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|n| n.is_finite() && *n > 0.0)
        {
            input.field_submissions.push(Control::Dimension(a, n));
        }
        let c = Control::Hug(a);
        let enabled = model.control_enabled(&c);
        let r = cell(
            ui,
            &format!("{axis} hug contents"),
            egui::vec2(chip_width, 20.0),
            hugging,
            enabled,
            theme::SURFACE_RAISED,
        );
        ui.painter().text(
            r.rect.center(),
            Align2::CENTER_CENTER,
            "Hug",
            chip_font,
            ink(hugging, enabled, true),
        );
        let tooltip = if !enabled {
            "wrap needs a width"
        } else if hugging {
            "hugging contents"
        } else {
            "hug contents"
        };
        if tip(r, tooltip).clicked() {
            input.clicked_controls.push(c);
        }
    });
}

fn dock_switch(ui: &mut egui::Ui, name: &str, width: f32, on: bool, enabled: bool) -> Response {
    card::switch(ui, name, width, on, enabled, None, |p, center, color| {
        paint_icon(p, center, name, color, 14.0);
    })
}
/// Paints a dock icon drawn on a 16 unit grid at `size` pixels; the stroke
/// scales with it so larger icons stay as crisp as the 14 pixel ones.
fn paint_icon(p: &egui::Painter, center: egui::Pos2, name: &str, color: Color32, size: f32) {
    let origin = center - Vec2::splat(size * 0.5);
    let point = |x: f32, y: f32| origin + egui::vec2(x, y) * (size / 16.0);
    let stroke = Stroke::new(1.25 * size / 14.0, color);
    let line = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let rect = |x: f32, y: f32, w: f32, h: f32| {
        p.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            0,
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
    // Alignment glyphs: a rule on the aligned side, dashed through the middle,
    // with two bars of different lengths set against it.
    let align = |vertical: bool, at: usize| {
        let rule = [(2.0, [2.0, 8.0, 14.0][at]), (14.0, [2.0, 8.0, 14.0][at])];
        let [a, b] = [
            [(4., 4., 3., 9.), (9., 4., 3., 6.)],
            [(4., 3., 3., 10.), (9., 5., 3., 6.)],
            [(4., 3., 3., 9.), (9., 6., 3., 6.)],
        ][at];
        let flip = |(x, y): (f32, f32)| if vertical { (y, x) } else { (x, y) };
        let rule = rule.map(flip);
        if at == 1 {
            dashed(&rule);
        } else {
            line(&rule);
        }
        for (x, y, w, h) in [a, b] {
            if vertical {
                rect(y, x, h, w);
            } else {
                rect(x, y, w, h);
            }
        }
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
        "Fill width" | "Fill height" => {
            let flip = |(x, y): (f32, f32)| {
                if name == "Fill height" {
                    (y, x)
                } else {
                    (x, y)
                }
            };
            for points in [
                &[(2., 3.), (2., 13.)][..],
                &[(14., 3.), (14., 13.)],
                &[(4., 8.), (12., 8.)],
                &[(6., 6.), (4., 8.), (6., 10.)],
                &[(10., 6.), (12., 8.), (10., 10.)],
            ] {
                line(&points.iter().map(|&p| flip(p)).collect::<Vec<_>>());
            }
        }
        "Align top" => align(false, 0),
        "Align middle" => align(false, 1),
        "Align bottom" => align(false, 2),
        "Align left" => align(true, 0),
        "Align center" => align(true, 1),
        "Align right" => align(true, 2),
        _ => {}
    }
}
