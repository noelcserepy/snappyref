use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::canvas::CanvasState;
use crate::data::{
    CanvasAnnotationGeometry, CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, RectGeometry,
};
use crate::library::storage::Library;

// The clipboard kind string is compatibility data. The typed payload below
// distinguishes Placements from Annotations even though old JSON says "items".
pub(crate) const COPY_PAYLOAD_KIND: &str = "snappyref-board-items";

pub(crate) struct ActiveBoardCommandContext<'a> {
    pub(crate) canvas: &'a mut CanvasState,
    pub(crate) library: Option<&'a Library>,
    pub(crate) import_worker_available: bool,
    pub(crate) placement_gap: f32,
    pub(crate) viewport_world: egui::Rect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopyCommandOutcome {
    pub(crate) payload_json: String,
    pub(crate) item_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PasteCommandOutcome {
    pub(crate) status: String,
    pub(crate) needs_existing_reload: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardBoardSelection {
    pub(crate) kind: String,
    pub(crate) source_center_x: f32,
    pub(crate) source_center_y: f32,
    #[serde(rename = "items")]
    pub(crate) placements: Vec<ClipboardPlacementState>,
    pub(crate) annotations: Vec<ClipboardAnnotation>,
    #[serde(default)]
    pub(crate) stack_order: Vec<ClipboardStackEntry>,
    #[serde(default)]
    pub(crate) groups: Vec<ClipboardGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardGroup {
    name: String,
    members: Vec<usize>,
    container: [f32; 4],
    flow: crate::data::GroupFlow,
    spacing: f32,
    padding: f32,
}
impl ClipboardGroup {
    fn bounds(&self) -> egui::Rect {
        egui::Rect::from_center_size(
            egui::pos2(self.container[0], self.container[1]),
            egui::vec2(self.container[2], self.container[3]),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardStackEntry {
    pub(crate) kind: String,
    pub(crate) index: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardPlacementState {
    asset_id: i64,
    center_x: f32,
    center_y: f32,
    scale: f32,
    rotation: f32,
    z_index: i32,
    width: f32,
    height: f32,
}

impl ClipboardPlacementState {
    fn geometry(&self) -> RectGeometry {
        RectGeometry::new(
            egui::pos2(self.center_x, self.center_y),
            egui::vec2(self.width, self.height),
            self.scale,
            self.rotation,
            self.z_index,
        )
    }

    fn translated_geometry(&self, delta: egui::Vec2) -> RectGeometry {
        let mut geometry = self.geometry();
        geometry.center += delta;
        geometry
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardAnnotation {
    kind: String,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    text: String,
    text_size: String,
    stroke_width: String,
    color: String,
    background_enabled: bool,
}

impl ClipboardAnnotation {
    fn geometry(&self) -> Result<CanvasAnnotationGeometry, String> {
        let kind = CanvasAnnotationKind::from_str(&self.kind)
            .ok_or_else(|| format!("unknown copied annotation kind: {}", self.kind))?;
        Ok(CanvasAnnotationGeometry::from_kind(
            kind,
            egui::pos2(self.start_x, self.start_y),
            egui::pos2(self.end_x, self.end_y),
        ))
    }

    fn translated_geometry(
        &self,
        kind: CanvasAnnotationKind,
        delta: egui::Vec2,
    ) -> CanvasAnnotationGeometry {
        CanvasAnnotationGeometry::from_kind(
            kind,
            egui::pos2(self.start_x, self.start_y) + delta,
            egui::pos2(self.end_x, self.end_y) + delta,
        )
    }
}

pub(crate) fn copy_selection_to_json(
    canvas: &CanvasState,
) -> Result<Option<CopyCommandOutcome>, String> {
    let Some(payload) = copy_selected_board_payload(canvas) else {
        return Ok(None);
    };
    let item_count = payload.placements.len() + payload.annotations.len() + payload.groups.len();
    let payload_json = serde_json::to_string(&payload)
        .map_err(|error| format!("Failed to serialize clipboard payload: {error}"))?;
    Ok(Some(CopyCommandOutcome {
        payload_json,
        item_count,
    }))
}

pub(crate) fn copy_selected_board_payload(canvas: &CanvasState) -> Option<ClipboardBoardSelection> {
    let source_bounds = canvas.selected_world_bounds()?;
    let mut placements = Vec::new();
    let mut placement_ids = Vec::new();
    for placement_id in canvas.selected_placement_ids() {
        let placement = canvas.placement_for_id(placement_id)?;
        let asset = placement.loaded_asset()?;
        let asset_id = asset.id;
        placement_ids.push(placement_id);
        placements.push(ClipboardPlacementState {
            asset_id,
            center_x: placement.state.geometry.center.x,
            center_y: placement.state.geometry.center.y,
            scale: placement.state.geometry.scale,
            rotation: placement.state.geometry.rotation,
            z_index: placement.state.geometry.z_index,
            width: placement.state.geometry.size.x,
            height: placement.state.geometry.size.y,
        });
    }

    let mut annotations = Vec::new();
    let mut annotation_ids = Vec::new();
    for annotation_id in canvas.selected_annotation_ids() {
        let annotation = canvas.annotation_for_id(annotation_id)?;
        annotation_ids.push(annotation_id);
        annotations.push(ClipboardAnnotation {
            kind: annotation.kind.as_str().to_owned(),
            start_x: annotation.start().x,
            start_y: annotation.start().y,
            end_x: annotation.end().x,
            end_y: annotation.end().y,
            text: annotation.text,
            text_size: annotation.text_size.as_str().to_owned(),
            stroke_width: annotation.stroke_width.as_str().to_owned(),
            color: annotation.color.as_str().to_owned(),
            background_enabled: annotation.background_enabled,
        });
    }

    let stack_order = canvas
        .canvas_stack_order()
        .into_iter()
        .filter_map(|item| match item {
            CanvasStackItem::Placement(placement_id) => placement_ids
                .iter()
                .position(|id| *id == placement_id)
                .map(|index| ClipboardStackEntry {
                    kind: "placement".to_owned(),
                    index,
                }),
            CanvasStackItem::Annotation(annotation_id) => annotation_ids
                .iter()
                .position(|id| *id == annotation_id)
                .map(|index| ClipboardStackEntry {
                    kind: "annotation".to_owned(),
                    index,
                }),
        })
        .collect();

    let groups = canvas
        .groups()
        .iter()
        .filter(|g| canvas.selected_group_ids().contains(&g.id))
        .filter_map(|group| {
            Some(ClipboardGroup {
                name: group.name.clone(),
                members: group
                    .members
                    .iter()
                    .map(|id| placement_ids.iter().position(|member| member == id))
                    .collect::<Option<Vec<_>>>()?,
                container: [
                    group.container.center.x,
                    group.container.center.y,
                    group.container.scaled_size().x,
                    group.container.scaled_size().y,
                ],
                flow: group.flow,
                spacing: group.spacing,
                padding: group.padding,
            })
        })
        .collect::<Vec<_>>();
    (!placements.is_empty() || !annotations.is_empty() || !groups.is_empty()).then(|| {
        ClipboardBoardSelection {
            kind: COPY_PAYLOAD_KIND.to_owned(),
            source_center_x: source_bounds.center().x,
            source_center_y: source_bounds.center().y,
            placements,
            annotations,
            stack_order,
            groups,
        }
    })
}

pub(crate) fn paste_json_at(
    context: &mut ActiveBoardCommandContext<'_>,
    paste_text: &str,
    anchor_world: egui::Pos2,
) -> Result<Option<PasteCommandOutcome>, String> {
    let Ok(payload) = serde_json::from_str::<ClipboardBoardSelection>(paste_text) else {
        return Ok(None);
    };
    if payload.kind != COPY_PAYLOAD_KIND
        || (payload.placements.is_empty()
            && payload.annotations.is_empty()
            && payload.groups.is_empty())
    {
        return Ok(None);
    }

    paste_board_payload_at(context, &payload, anchor_world).map(Some)
}

pub(crate) fn paste_board_payload_at(
    context: &mut ActiveBoardCommandContext<'_>,
    payload: &ClipboardBoardSelection,
    requested_anchor_world: egui::Pos2,
) -> Result<PasteCommandOutcome, String> {
    validate_clipboard_payload(payload)?;
    let Some(library) = context.library else {
        return Err("Storage unavailable".to_owned());
    };
    let source_center = egui::pos2(payload.source_center_x, payload.source_center_y);
    let target_anchor_world = placement_anchor_for_payload(
        library,
        context.viewport_world,
        context.placement_gap,
        payload,
        requested_anchor_world,
    )?;
    let delta = target_anchor_world - source_center;
    let mut needs_existing_reload = false;

    let mut pasted_placement_ids = Vec::new();
    for placement in &payload.placements {
        let geometry = placement.translated_geometry(delta);
        let mut placement = library.new_placement_from_asset(
            placement.asset_id,
            geometry.center,
            geometry.scale,
            geometry.rotation,
            geometry.z_index,
        )?;
        // Display geometry belongs to the Placement, independently of Asset pixels.
        placement.state.geometry = geometry;
        pasted_placement_ids.push(placement.state.id);
        let placement_id = placement.state.id;
        if let Err(error) = context
            .canvas
            .add_loaded_reference_placement(placement.clone())
        {
            if context.import_worker_available {
                context.canvas.add_unresolved_placement(placement);
                needs_existing_reload = true;
            } else {
                return Err(error);
            }
        }
        context.canvas.mark_placement_for_persistence(placement_id);
    }

    let mut pasted_annotation_ids = Vec::new();
    for annotation in &payload.annotations {
        let kind = CanvasAnnotationKind::from_str(&annotation.kind)
            .ok_or_else(|| format!("unknown copied annotation kind: {}", annotation.kind))?;
        let text_size = CanvasTextSize::from_str(&annotation.text_size)
            .ok_or_else(|| format!("unknown copied text size: {}", annotation.text_size))?;
        let stroke_width = CanvasStrokeWidth::from_str(&annotation.stroke_width)
            .ok_or_else(|| format!("unknown copied stroke width: {}", annotation.stroke_width))?;
        let color = crate::data::CanvasAnnotationColor::from_str(&annotation.color)
            .ok_or_else(|| format!("unknown copied annotation color: {}", annotation.color))?;
        let geometry = annotation.translated_geometry(kind, delta);
        let pasted = library.new_canvas_annotation(
            kind,
            geometry.start(),
            geometry.end(),
            annotation.text.clone(),
            text_size,
            stroke_width,
            color,
            annotation.background_enabled,
        );
        pasted_annotation_ids.push(pasted.id);
        context.canvas.mark_annotation_for_persistence(pasted.id);
        context.canvas.add_annotation(pasted);
    }

    let mut groups = context.canvas.groups().to_vec();
    let mut pasted_group_ids = Vec::new();
    for group in &payload.groups {
        let id = library.allocate_document_id();
        pasted_group_ids.push(id);
        groups.push(crate::data::Group {
            id,
            name: group.name.clone(),
            members: group
                .members
                .iter()
                .map(|index| pasted_placement_ids[*index])
                .collect(),
            container: RectGeometry::new(
                group.bounds().center() + delta,
                group.bounds().size(),
                1.,
                0.,
                0,
            ),
            flow: group.flow,
            spacing: group.spacing,
            padding: group.padding,
        });
    }
    context.canvas.replace_groups(groups);
    context
        .canvas
        .select_content(&pasted_placement_ids, &pasted_annotation_ids);
    context.canvas.select_pasted_groups(pasted_group_ids);
    move_pasted_content_to_front(
        context.canvas,
        &pasted_placement_ids,
        &pasted_annotation_ids,
        payload,
    );
    let count = pasted_placement_ids.len() + pasted_annotation_ids.len() + payload.groups.len();
    let noun = if count == 1 { "item" } else { "items" };
    Ok(PasteCommandOutcome {
        status: format!("Pasted {count} {noun}"),
        needs_existing_reload,
    })
}

pub(crate) fn duplicate_selection(
    context: &mut ActiveBoardCommandContext<'_>,
) -> Result<Option<PasteCommandOutcome>, String> {
    let Some(payload) = copy_selected_board_payload(context.canvas) else {
        return Ok(None);
    };
    let Some(bounds) = clipboard_payload_bounds(&payload) else {
        return Ok(None);
    };
    let offset = egui::vec2(
        context.placement_gap.max(12.0),
        context.placement_gap.max(12.0),
    );
    let mut outcome = paste_board_payload_at(context, &payload, bounds.center() + offset)?;
    outcome.status = outcome.status.replacen("Pasted", "Duplicated", 1);
    Ok(Some(outcome))
}

pub(crate) fn delete_selected_from_board(
    context: &mut ActiveBoardCommandContext<'_>,
) -> Result<Option<String>, String> {
    let mut status = None;
    let annotation_ids = context.canvas.delete_selected_annotations();
    if !annotation_ids.is_empty() {
        let noun = if annotation_ids.len() == 1 {
            "annotation"
        } else {
            "annotations"
        };
        status = Some(format!("Deleted {} {noun}", annotation_ids.len()));
    }

    let archived_ids = context.canvas.delete_selected_placements();
    if archived_ids.is_empty() {
        return Ok(status);
    }

    let noun = if archived_ids.len() == 1 {
        "image"
    } else {
        "images"
    };
    status = Some(format!("Deleted {} {noun} from board", archived_ids.len()));
    Ok(status)
}

pub(crate) fn move_selected_to_front(
    context: &mut ActiveBoardCommandContext<'_>,
) -> Result<bool, String> {
    let changed = context.canvas.move_selected_canvas_items_to_front();
    Ok(changed)
}

pub(crate) fn move_selected_to_back(
    context: &mut ActiveBoardCommandContext<'_>,
) -> Result<bool, String> {
    let changed = context.canvas.move_selected_canvas_items_to_back();
    Ok(changed)
}

fn move_pasted_content_to_front(
    canvas: &mut CanvasState,
    pasted_placement_ids: &[i64],
    pasted_annotation_ids: &[i64],
    payload: &ClipboardBoardSelection,
) {
    let mut pasted_stack_order = Vec::new();
    for entry in &payload.stack_order {
        match entry.kind.as_str() {
            "placement" => {
                if let Some(placement_id) = pasted_placement_ids.get(entry.index) {
                    pasted_stack_order.push(CanvasStackItem::Placement(*placement_id));
                }
            }
            "annotation" => {
                if let Some(annotation_id) = pasted_annotation_ids.get(entry.index) {
                    pasted_stack_order.push(CanvasStackItem::Annotation(*annotation_id));
                }
            }
            _ => {}
        }
    }
    if pasted_stack_order.len() != pasted_placement_ids.len() + pasted_annotation_ids.len() {
        pasted_stack_order = pasted_placement_ids
            .iter()
            .copied()
            .map(CanvasStackItem::Placement)
            .chain(
                pasted_annotation_ids
                    .iter()
                    .copied()
                    .map(CanvasStackItem::Annotation),
            )
            .collect();
    }

    let mut next_stack_order = canvas.canvas_stack_order();
    next_stack_order.retain(|item| !pasted_stack_order.contains(item));
    next_stack_order.extend(pasted_stack_order);
    canvas.set_canvas_stack_order(next_stack_order);
}

fn placement_anchor_for_payload(
    library: &Library,
    viewport_world: egui::Rect,
    placement_gap: f32,
    payload: &ClipboardBoardSelection,
    requested_anchor_world: egui::Pos2,
) -> Result<egui::Pos2, String> {
    let Some(bounds) = clipboard_payload_bounds(payload) else {
        return Ok(requested_anchor_world);
    };
    crate::jobs::arrange_rect_world(
        library,
        requested_anchor_world,
        viewport_world,
        bounds.width(),
        bounds.height(),
        placement_gap,
    )
}

pub(crate) fn clipboard_payload_bounds(payload: &ClipboardBoardSelection) -> Option<egui::Rect> {
    let mut bounds: Option<egui::Rect> = None;

    for group in &payload.groups {
        bounds = Some(bounds.map_or(group.bounds(), |existing| existing.union(group.bounds())));
    }
    for placement in &payload.placements {
        let rect = placement.geometry().unrotated_bounds();
        bounds = Some(match bounds {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    for annotation in &payload.annotations {
        let Ok(geometry) = annotation.geometry() else {
            return None;
        };
        let rect = egui::Rect::from_two_pos(geometry.start(), geometry.end());
        bounds = Some(match bounds {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    bounds
}

fn validate_clipboard_payload(payload: &ClipboardBoardSelection) -> Result<(), String> {
    let invalid = || "Invalid SnappyRef clipboard metadata".to_owned();
    if payload.kind != COPY_PAYLOAD_KIND
        || !payload.source_center_x.is_finite()
        || !payload.source_center_y.is_finite()
    {
        return Err(invalid());
    }
    let mut membership = std::collections::HashSet::new();
    for group in &payload.groups {
        if group.container.iter().any(|n| !n.is_finite())
            || group.container[2] <= 0.
            || group.container[3] <= 0.
            || !group.spacing.is_finite()
            || group.spacing < 0.
            || !group.padding.is_finite()
            || group.padding < 0.
            || group.padding * 2. >= group.container[2]
        {
            return Err(invalid());
        }
        for index in &group.members {
            if *index >= payload.placements.len() || !membership.insert(*index) {
                return Err(invalid());
            }
        }
    }
    for placement in &payload.placements {
        if [
            placement.center_x,
            placement.center_y,
            placement.scale,
            placement.rotation,
        ]
        .iter()
        .any(|n| !n.is_finite())
            || placement.scale <= 0.
            || !placement.width.is_finite()
            || !placement.height.is_finite()
            || placement.width <= 0.
            || placement.height <= 0.
        {
            return Err(invalid());
        }
    }
    for annotation in &payload.annotations {
        if [
            annotation.start_x,
            annotation.start_y,
            annotation.end_x,
            annotation.end_y,
        ]
        .iter()
        .any(|n| !n.is_finite())
            || annotation.geometry().is_err()
            || CanvasTextSize::from_str(&annotation.text_size).is_none()
            || CanvasStrokeWidth::from_str(&annotation.stroke_width).is_none()
            || crate::data::CanvasAnnotationColor::from_str(&annotation.color).is_none()
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod group_tests {
    use super::*;

    #[test]
    fn group_clipboard_remaps_members_between_boards_and_contains_invalid_metadata() {
        let root = crate::test_support::TempRoot::new("group-clipboard");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let mut source = CanvasState::default();
        let path = library.store_rgba_image("group", &[255; 16], 2, 2).unwrap();
        let first = library
            .persist_imported_image(&path, "image/png", 2, 2, 16, egui::pos2(0., 0.), 1.)
            .unwrap();
        let second = library
            .new_placement_from_asset(first.asset_id().unwrap(), egui::pos2(100., 0.), 1., 0., 1)
            .unwrap();
        let ids = [first.id(), second.id()];
        source.add_unresolved_placement(first);
        source.add_unresolved_placement(second);
        source.select_content(&ids, &[]);
        let source_id = source
            .create_group(library.allocate_document_id())
            .unwrap()
            .unwrap();
        let payload = copy_selection_to_json(&source).unwrap().unwrap();
        let target_board = library.create_board("Clipboard destination").unwrap();
        library.set_active_board(target_board.id).unwrap();
        let mut target = CanvasState::default();
        let mut context = ActiveBoardCommandContext {
            canvas: &mut target,
            library: Some(&library),
            import_worker_available: true,
            placement_gap: 16.,
            viewport_world: egui::Rect::from_center_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 1000.),
            ),
        };
        paste_json_at(&mut context, &payload.payload_json, egui::pos2(400., 300.))
            .unwrap()
            .unwrap();
        let group = &context.canvas.groups()[0];
        assert_ne!(group.id, source_id);
        assert!(group.members.iter().all(|id| !ids.contains(id)));
        assert_eq!(group.members, context.canvas.selected_placement_ids());
        assert_eq!(group.flow, source.groups()[0].flow);
        let delta = group.container.center - source.groups()[0].container.center;
        for (source_id, target_id) in ids.iter().zip(&group.members) {
            assert_eq!(
                context
                    .canvas
                    .placement_for_id(*target_id)
                    .unwrap()
                    .state
                    .geometry
                    .center,
                source
                    .placement_for_id(*source_id)
                    .unwrap()
                    .state
                    .geometry
                    .center
                    + delta
            );
        }
        let mut malformed: ClipboardBoardSelection =
            serde_json::from_str(&payload.payload_json).unwrap();
        malformed.groups[0].members = vec![0, 0];
        let before = context.canvas.all_placement_ids();
        assert!(paste_board_payload_at(&mut context, &malformed, egui::Pos2::ZERO).is_err());
        assert_eq!(context.canvas.all_placement_ids(), before);
        source.delete_placements(&ids);
        source.select_group(source_id);
        let empty = copy_selection_to_json(&source).unwrap().unwrap();
        paste_json_at(&mut context, &empty.payload_json, egui::pos2(600., 300.))
            .unwrap()
            .unwrap();
        assert_eq!(context.canvas.groups().len(), 2);
        assert!(context.canvas.groups()[1].members.is_empty());
        assert_ne!(context.canvas.groups()[1].id, source_id);
    }

    #[test]
    fn fitted_and_scaled_group_members_keep_display_geometry_through_paste_and_duplicate() {
        let root = crate::test_support::TempRoot::new("group-clipboard-display-geometry");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library
            .store_rgba_image("wide", &vec![255; 200 * 100 * 4], 200, 100)
            .unwrap();
        let placement = library
            .persist_imported_image(&path, "image/png", 200, 100, 80000, egui::Pos2::ZERO, 1.)
            .unwrap();
        for factor in [1., 1.375] {
            let mut source = CanvasState::default();
            source.add_unresolved_placement(placement.clone());
            let group_id = library.allocate_document_id();
            source.restore_groups(vec![crate::data::Group {
                id: group_id,
                name: "Fitted".into(),
                members: vec![],
                container: RectGeometry::new(
                    egui::pos2(80., 60.),
                    egui::vec2(120., 100.),
                    1.,
                    0.,
                    0,
                ),
                flow: crate::data::GroupFlow::Wrap,
                spacing: 7.,
                padding: 10.,
            }]);
            source
                .add_members_to_group(group_id, &[placement.id()])
                .unwrap();
            assert_eq!(
                source
                    .placement_for_id(placement.id())
                    .unwrap()
                    .scaled_display_size(),
                egui::vec2(100., 50.)
            );
            let layout = crate::placement::group_layout::scale_group(
                &source.groups()[0],
                &[(
                    placement.id(),
                    source
                        .placement_for_id(placement.id())
                        .unwrap()
                        .state
                        .geometry,
                )],
                factor,
            )
            .unwrap();
            let mut scaled = source.placement_for_id(placement.id()).unwrap();
            scaled.state.geometry = layout.members[0].1;
            source.apply_placement_history_state(Some(scaled));
            let mut group = source.groups()[0].clone();
            group.container = layout.container;
            group.spacing = layout.spacing;
            group.padding = layout.padding;
            source.replace_groups(vec![group]);
            source.select_group(group_id);
            let expected = source
                .placement_for_id(placement.id())
                .unwrap()
                .state
                .geometry;
            let payload = copy_selection_to_json(&source).unwrap().unwrap();
            let mut target = CanvasState::default();
            let viewport = egui::Rect::from_center_size(egui::Pos2::ZERO, egui::vec2(2000., 2000.));
            let mut target_context = ActiveBoardCommandContext {
                canvas: &mut target,
                library: Some(&library),
                import_worker_available: true,
                placement_gap: 16.,
                viewport_world: viewport,
            };
            paste_json_at(
                &mut target_context,
                &payload.payload_json,
                egui::pos2(400., 300.),
            )
            .unwrap()
            .unwrap();
            let pasted_group = &target_context.canvas.groups()[0];
            let pasted = target_context
                .canvas
                .placement_for_id(pasted_group.members[0])
                .unwrap();
            assert_eq!(
                pasted.state.geometry.size, expected.size,
                "paste after scale {factor}"
            );
            assert_eq!(pasted.state.geometry.scale, expected.scale);
            assert_eq!(
                pasted.state.geometry.center - pasted_group.container.center,
                expected.center - source.groups()[0].container.center
            );
            assert_eq!(pasted_group.padding, source.groups()[0].padding);
            assert_eq!(pasted_group.spacing, source.groups()[0].spacing);
            let mut source_context = ActiveBoardCommandContext {
                canvas: &mut source,
                library: Some(&library),
                import_worker_available: true,
                placement_gap: 16.,
                viewport_world: viewport,
            };
            duplicate_selection(&mut source_context).unwrap().unwrap();
            let duplicated_group = &source_context.canvas.groups()[1];
            let duplicated = source_context
                .canvas
                .placement_for_id(duplicated_group.members[0])
                .unwrap();
            assert_ne!(duplicated.id(), placement.id());
            assert_ne!(duplicated_group.id, group_id);
            assert_eq!(
                duplicated.state.geometry.size, expected.size,
                "duplicate after scale {factor}"
            );
            assert_eq!(duplicated.state.geometry.scale, expected.scale);
            assert_eq!(
                duplicated.state.geometry.center - duplicated_group.container.center,
                expected.center - source_context.canvas.groups()[0].container.center
            );
        }
    }

    #[test]
    fn transfer_closes_source_gap_and_keeps_destination_append_order() {
        let root = crate::test_support::TempRoot::new("group-transfer");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library.store_rgba_image("group", &[255; 16], 2, 2).unwrap();
        let first = library
            .persist_imported_image(&path, "image/png", 2, 2, 16, egui::pos2(0., 0.), 1.)
            .unwrap();
        let mut canvas = CanvasState::default();
        let mut ids = vec![first.id()];
        let asset = first.asset_id().unwrap();
        canvas.add_unresolved_placement(first);
        for i in 1..4 {
            let p = library
                .new_placement_from_asset(asset, egui::pos2(i as f32 * 100., 0.), 1., 0., i)
                .unwrap();
            ids.push(p.id());
            canvas.add_unresolved_placement(p);
        }
        canvas.select_content(&ids[..2], &[]);
        let a = canvas
            .create_group(library.allocate_document_id())
            .unwrap()
            .unwrap();
        let origin = canvas
            .placement_for_id(ids[0])
            .unwrap()
            .state
            .geometry
            .center;
        canvas.select_content(&ids[2..], &[]);
        let b = canvas
            .create_group(library.allocate_document_id())
            .unwrap()
            .unwrap();
        let existing = canvas.placement_for_id(ids[2]).unwrap().state.geometry;
        canvas.add_members_to_group(b, &[ids[0]]).unwrap();
        assert_eq!(
            canvas.groups().iter().find(|g| g.id == a).unwrap().members,
            vec![ids[1]]
        );
        assert_eq!(
            canvas
                .placement_for_id(ids[1])
                .unwrap()
                .state
                .geometry
                .center,
            origin
        );
        assert_eq!(
            canvas.placement_for_id(ids[2]).unwrap().state.geometry,
            existing
        );
        assert_eq!(
            canvas.groups().iter().find(|g| g.id == b).unwrap().members,
            vec![ids[2], ids[3], ids[0]]
        );
        assert!(!canvas.add_members_to_group(b, &[ids[0]]).unwrap());
    }
}
