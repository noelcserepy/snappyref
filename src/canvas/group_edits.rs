//! Canvas geometry/history adapter around the grouping state machine.
use super::CanvasState;
use crate::data::{CanvasAnnotationGeometry, Group, GroupFlow, MemberLayout, RectGeometry};
use crate::grouping::{Board, Member, MemberKind, Outcome, State};
use crate::placement::group_pixels::{Pixels, config};
use eframe::egui::{self, Vec2};

impl CanvasState {
    pub(super) fn group_member_geometry(&self, id: i64) -> Option<RectGeometry> {
        self.placement_by_id(id)
            .map(|p| p.state.geometry)
            .or_else(|| {
                self.annotation_by_id(id).and_then(|a| match a.geometry {
                    CanvasAnnotationGeometry::Rect(r) => Some(r),
                    _ => None,
                })
            })
    }
    pub(super) fn set_group_member_geometry(&mut self, id: i64, geometry: RectGeometry) {
        if let Some(p) = self
            .document
            .placements
            .iter_mut()
            .find(|p| p.state.id == id)
        {
            if p.state.geometry != geometry {
                p.state.geometry = geometry;
                self.mark_dirty(id);
            }
        } else if let Some(a) = self.document.annotations.iter_mut().find(|a| a.id == id)
            && a.geometry != CanvasAnnotationGeometry::Rect(geometry)
        {
            a.geometry = CanvasAnnotationGeometry::Rect(geometry);
            self.mark_annotation_for_persistence(id);
        }
    }
    fn group_image_size(&self, id: i64) -> Option<Vec2> {
        self.placement_for_id(id)?
            .loaded_asset()
            .map(|asset| egui::vec2(asset.width as f32, asset.height as f32))
    }
    pub(super) fn grouping_state(&self) -> State<Pixels> {
        let mut board = Board::default();
        let mut pixels = Pixels::default();
        for p in &self.document.placements {
            if p.lifecycle == crate::data::PlacementLifecycle::Committed {
                board.live.insert(p.state.id);
                board.members.insert(
                    p.state.id,
                    Member::fixed(
                        MemberKind::Image,
                        p.state.geometry.visual_bounds().size().into(),
                    ),
                );
                pixels.rects.insert(p.state.id, p.state.geometry);
                if let Some(asset) = p.loaded_asset() {
                    pixels.image_sizes.insert(
                        p.state.id,
                        crate::data::rotated_display_size(
                            egui::vec2(asset.width as f32, asset.height as f32),
                            p.state.geometry.rotation,
                        ),
                    );
                }
            }
        }
        for a in &self.document.annotations {
            if let CanvasAnnotationGeometry::Rect(r) = a.geometry {
                board.live.insert(a.id);
                board.members.insert(
                    a.id,
                    Member::fixed(MemberKind::Textbox, r.visual_bounds().size().into()),
                );
                pixels.rects.insert(a.id, r);
            }
        }
        for group in self.groups() {
            board.groups.insert(group.id, config(group));
            for &id in &group.members {
                if let Some(member) = board.members.get_mut(&id) {
                    let mode = group.layout.members.get(&id).copied().unwrap_or_default();
                    member.absolute = mode.absolute;
                    member.fill = mode.fill;
                }
            }
        }
        let mut state = State::init(board, pixels, self.placement_gap);
        state.dock = self.current_group_dock();
        state
    }
    pub(super) fn group_edit(
        &mut self,
        edit: impl FnOnce(&mut State<Pixels>) -> Outcome,
    ) -> Result<(), String> {
        let mut state = self.grouping_state();
        edit(&mut state)?;
        self.apply_group_state(state, None)
    }
    pub(super) fn apply_group_state(
        &mut self,
        state: State<Pixels>,
        new_group: Option<Group>,
    ) -> Result<(), String> {
        let previous = self.grouping_state();
        let mut groups = Vec::new();
        let mut geometries = state.geometry.rects.clone();
        for (&id, config) in &state.board.groups {
            let mut group = self
                .groups()
                .iter()
                .find(|g| g.id == id)
                .cloned()
                .or_else(|| new_group.as_ref().filter(|g| g.id == id).cloned())
                .ok_or("group_missing")?;
            let old_origin = group.container.unrotated_bounds().min;
            let old_padding = group.padding;
            group.name = config.name.clone();
            group.members = config.order.clone();
            group.flow = config.flow;
            group.gap = config.gap;
            group.padding = config.padding;
            group.layout.fixed = config.fixed;
            group.layout.auto = config.auto;
            group.layout.row_gap = config.row_gap;
            group.layout.align = config.align;
            group.layout.clip = config.clip;
            group.layout.members = config
                .order
                .iter()
                .map(|&m| {
                    let member = &state.board.members[&m];
                    (
                        m,
                        MemberLayout {
                            fill: member.fill,
                            absolute: member.absolute,
                        },
                    )
                })
                .filter(|(_, mode)| *mode != MemberLayout::default())
                .collect();
            let mut origin = group.container.unrotated_bounds().min;
            let size = Vec2::from(config.size);
            let layout = state.geometry.layout(&state.board, id);
            let mut content_origin = origin + Vec2::splat(group.padding);
            if group.flow != GroupFlow::Free {
                let old_min = if previous.board.groups.contains_key(&id) {
                    previous.geometry.layout(&previous.board, id).min
                } else {
                    Vec2::ZERO
                };
                for a in 0..2 {
                    if !group.layout.fixed[a] {
                        content_origin[a] = old_origin[a] + old_padding - old_min[a];
                        origin[a] = content_origin[a] + layout.min[a] - group.padding;
                    }
                }
            }

            if group.flow == GroupFlow::Free
                && let Some(bounds) = state
                    .board
                    .flowing(id)
                    .iter()
                    .filter_map(|m| geometries.get(m))
                    .map(|r| r.visual_bounds())
                    .reduce(|a, b| a.union(b))
            {
                for a in 0..2 {
                    if !group.layout.fixed[a] {
                        origin[a] = bounds.min[a] - group.padding;
                    }
                }
            }
            group.container = RectGeometry::from_bounds(
                egui::Rect::from_min_size(origin, size),
                group.container.z_index,
            );
            for &m in &config.order {
                let member = &state.board.members[&m];
                let Some(rect) = geometries.get_mut(&m) else {
                    continue;
                };
                let desired = layout.sizes[&m];
                resize_observation(
                    rect,
                    desired,
                    member.kind == MemberKind::Image,
                    self.group_image_size(m),
                );
                if member.absolute {
                    rect.center += origin - old_origin;
                } else if group.flow != GroupFlow::Free {
                    rect.center = content_origin + layout.offsets[&m] + desired * 0.5;
                }
            }
            groups.push(group);
        }
        // Detach/ungroup freeze their observed size before removing membership.
        for (&id, member) in &state.board.members {
            if state.board.owner(id).is_none()
                && self.group_for_member(id).is_some()
                && let Some(rect) = geometries.get_mut(&id)
            {
                resize_observation(
                    rect,
                    Vec2::from(member.size),
                    member.kind == MemberKind::Image,
                    self.group_image_size(id),
                );
            }
        }
        for (id, geometry) in geometries {
            self.set_group_member_geometry(id, geometry);
        }
        let affected = groups
            .iter()
            .filter(|group| self.groups().iter().find(|old| old.id == group.id) != Some(*group))
            .flat_map(|group| group.members.iter().copied())
            .collect::<Vec<_>>();
        for id in affected {
            if self.placement_for_id(id).is_some() {
                self.mark_dirty(id);
            } else {
                self.mark_annotation_for_persistence(id);
            }
        }
        self.replace_groups(groups);
        self.accept_group_dock(state.dock);

        Ok(())
    }
}
fn resize_observation(
    rect: &mut RectGeometry,
    desired: Vec2,
    image: bool,
    asset_size: Option<Vec2>,
) {
    if desired.min_elem() <= 0.0 {
        rect.size = desired.max(Vec2::ZERO) / rect.scale;
        return;
    }
    if image && rect.size.min_elem() == 0.0 {
        // Zero Fill erased the ratio. Until the asset loads, nothing can restore it.
        let Some(size) = asset_size else {
            return;
        };
        rect.size = size;
    }
    let current = rect.visual_bounds().size();
    if image {
        let factor = desired.x / current.x;
        rect.size *= factor;
    } else {
        rect.size = desired / rect.scale;
    }
}

impl CanvasState {
    /// The dock decides the paste destination: after the edited member, appended
    /// to the shown Group, or loose while hidden. Member -1 means no member.
    pub(crate) fn group_paste_context(&self) -> Option<(i64, i64)> {
        let dock = self.current_group_dock();
        Some((dock.group()?, dock.member().unwrap_or(-1)))
    }
    /// Admits fresh items in order. An item with a source is a duplicate of it.
    pub(crate) fn paste_group_members(
        &mut self,
        context: Option<(i64, i64)>,
        members: &[(i64, Option<i64>)],
    ) -> Result<(), String> {
        if context.is_none() && members.iter().all(|(_, source)| source.is_none()) {
            return Ok(());
        }
        let mut state = self.grouping_state();
        match context {
            Some((g, -1)) => state.select_group(g)?,
            Some((_, anchor)) => state.edit_member(anchor)?,
            None => state.clear_selection(),
        }
        for &(id, _) in members {
            if let Some(member) = state.board.members.get(&id).cloned() {
                state.geometry.intake.insert(id, member);
                state.board.live.remove(&id);
            }
        }
        for &(id, source) in members {
            let Some(member) = state.geometry.intake.get(&id).cloned() else {
                continue;
            };
            match source {
                Some(source) => state.duplicate(source, id)?,
                None => state.paste(id, member.size[0])?,
            }
        }
        self.apply_group_state(state, None)
    }
}

impl CanvasState {
    /// Preparation replaces provisional dimensions at the geometry boundary. Reapply the
    /// arrival in its existing slot, so final pixels receive the same incoming-fit rule.
    pub(crate) fn finish_group_arrival(&mut self, m: i64) -> Result<(), String> {
        let Some(g) = self.group_for_member(m) else {
            return Ok(());
        };
        let mut state = self.grouping_state();
        let order = &mut state.board.groups.get_mut(&g).unwrap().order;
        let slot = order.iter().position(|&id| id == m).unwrap();
        order.remove(slot);
        state.drag_in(g, m, slot as i64)?;
        self.apply_group_state(state, None)
    }
}

#[cfg(test)]
#[path = "group_storage_tests.rs"]
mod storage_tests;

#[cfg(test)]
#[path = "group_geometry_tests.rs"]
mod geometry_tests;
