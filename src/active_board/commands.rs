#[cfg(test)]
use crate::sources::clipboard::decode_board_selection;
use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::{
    CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, RectGeometry,
};
use crate::library::storage::Library;
use crate::sources::clipboard::{
    ClipboardBoardSelection, clipboard_payload_bounds, copy_selected_board_payload,
    validate_clipboard_payload,
};

pub(crate) struct ActiveBoardCommandContext<'a> {
    pub(crate) canvas: &'a mut CanvasState,
    pub(crate) library: Option<&'a Library>,
    pub(crate) import_worker_available: bool,
    pub(crate) placement_gap: f32,
    pub(crate) viewport_world: egui::Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct PasteCommandOutcome {
    pub(crate) status: String,
    pub(super) reload: Vec<crate::data::Placement>,
}

#[cfg(test)]
pub(crate) fn paste_json_at(
    context: &mut ActiveBoardCommandContext<'_>,
    paste_text: &str,
    anchor_world: egui::Pos2,
) -> Result<Option<PasteCommandOutcome>, String> {
    let Some(payload) = decode_board_selection(paste_text)? else {
        return Ok(None);
    };

    paste_board_payload_at(context, &payload, anchor_world).map(Some)
}

pub(crate) fn paste_board_payload_at(
    context: &mut ActiveBoardCommandContext<'_>,
    payload: &ClipboardBoardSelection,
    requested_anchor_world: egui::Pos2,
) -> Result<PasteCommandOutcome, String> {
    paste_payload(context, payload, requested_anchor_world, None)
}
fn paste_payload(
    context: &mut ActiveBoardCommandContext<'_>,
    payload: &ClipboardBoardSelection,
    requested_anchor_world: egui::Pos2,
    // Duplicate sources in payload order: selected placements, then annotations.
    sources: Option<&[i64]>,
) -> Result<PasteCommandOutcome, String> {
    validate_clipboard_payload(payload)?;
    let group_context = context.canvas.group_paste_context();
    let Some(library) = context.library else {
        return Err("Storage unavailable".to_owned());
    };
    let source_center = egui::pos2(payload.source_center_x, payload.source_center_y);
    let target_anchor_world = placement_anchor_for_payload(
        context.canvas,
        context.viewport_world,
        context.placement_gap,
        payload,
        requested_anchor_world,
    )?;
    let delta = target_anchor_world - source_center;
    let mut reload = Vec::new();

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
                context.canvas.add_unresolved_placement(placement.clone());
                reload.push(placement);
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
        let fill = crate::data::CanvasRectangleFill::from_str(&annotation.fill)
            .ok_or_else(|| format!("unknown copied rectangle fill: {}", annotation.fill))?;
        let geometry = annotation.translated_geometry(kind, delta);
        let mut pasted = library.new_canvas_annotation(
            kind,
            geometry.start(),
            geometry.end(),
            annotation.text.clone(),
            text_size,
            stroke_width,
            color,
        );
        pasted.fill = fill;
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
                .map(|member| {
                    if member.kind == "placement" {
                        pasted_placement_ids[member.index]
                    } else {
                        pasted_annotation_ids[member.index]
                    }
                })
                .collect(),
            container: RectGeometry::new(
                group.bounds().center() + delta,
                group.bounds().size(),
                1.,
                0.,
                0,
            ),
            flow: group.flow,
            layout: {
                let mut layout = group.layout.clone();
                layout.members = group
                    .members
                    .iter()
                    .enumerate()
                    .filter_map(|(i, member)| {
                        group.layout.members.get(&(i as i64)).copied().map(|mode| {
                            (
                                if member.kind == "placement" {
                                    pasted_placement_ids[member.index]
                                } else {
                                    pasted_annotation_ids[member.index]
                                },
                                mode,
                            )
                        })
                    })
                    .collect();
                layout
            },
            gap: group.gap,
            padding: group.padding,
        });
    }
    context.canvas.replace_groups(groups);
    context
        .canvas
        .select_content(&pasted_placement_ids, &pasted_annotation_ids);
    context.canvas.select_pasted_groups(pasted_group_ids);
    let created = pasted_stack_order(&pasted_placement_ids, &pasted_annotation_ids, payload);
    // Copied Groups already own their pasted members.
    if payload.groups.is_empty() {
        let fresh = pasted_placement_ids
            .iter()
            .chain(&pasted_annotation_ids)
            .copied()
            .collect::<Vec<_>>();
        let members = created
            .iter()
            .map(|item| {
                let id = item.item_id();
                let source = sources.map(|sources| {
                    sources[fresh
                        .iter()
                        .position(|&f| f == id)
                        .expect("created is fresh")]
                });
                (id, source)
            })
            .collect::<Vec<_>>();
        context
            .canvas
            .paste_group_members(group_context, &members)?;
    }
    let mut next_stack_order = context.canvas.canvas_stack_order();
    next_stack_order.retain(|item| !created.contains(item));
    next_stack_order.extend(created.iter().copied());
    context.canvas.set_canvas_stack_order(next_stack_order);
    let count = pasted_placement_ids.len() + pasted_annotation_ids.len() + payload.groups.len();
    let noun = if count == 1 { "item" } else { "items" };
    Ok(PasteCommandOutcome {
        status: format!("Pasted {count} {noun}"),
        reload,
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
    let sources = context
        .canvas
        .selected_placement_ids()
        .into_iter()
        .chain(context.canvas.selected_annotation_ids())
        .collect::<Vec<_>>();
    let mut outcome = paste_payload(context, &payload, bounds.center() + offset, Some(&sources))?;
    outcome.status = outcome.status.replacen("Pasted", "Duplicated", 1);
    Ok(Some(outcome))
}

pub(crate) fn delete_selected_from_board(
    context: &mut ActiveBoardCommandContext<'_>,
) -> Result<Option<String>, String> {
    let mut status = None;
    let selected_groups = context.canvas.selected_group_ids().len();
    if selected_groups > 0 {
        context.canvas.ungroup_selected();
        let noun = if selected_groups == 1 {
            "group"
        } else {
            "groups"
        };
        status = Some(format!("Deleted {selected_groups} {noun}"));
    }
    let placement_ids = context.canvas.selected_placement_ids();
    let annotation_ids = context.canvas.delete_selected_annotations();
    if !annotation_ids.is_empty() {
        let noun = if annotation_ids.len() == 1 {
            "annotation"
        } else {
            "annotations"
        };
        status = Some(format!("Deleted {} {noun}", annotation_ids.len()));
    }

    let archived_ids = context.canvas.delete_placements(&placement_ids);
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

fn pasted_stack_order(
    pasted_placement_ids: &[i64],
    pasted_annotation_ids: &[i64],
    payload: &ClipboardBoardSelection,
) -> Vec<CanvasStackItem> {
    let mut pasted_stack_order = Vec::new();
    for entry in &payload.stack_order {
        let item = match entry.kind.as_str() {
            "placement" => pasted_placement_ids
                .get(entry.index)
                .map(|&id| CanvasStackItem::Placement(id)),
            "annotation" => pasted_annotation_ids
                .get(entry.index)
                .map(|&id| CanvasStackItem::Annotation(id)),
            _ => None,
        };
        // Repeated entries would leave another pasted item out of the order and its history.
        if let Some(item) = item.filter(|item| !pasted_stack_order.contains(item)) {
            pasted_stack_order.push(item);
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
    pasted_stack_order
}

fn placement_anchor_for_payload(
    canvas: &CanvasState,
    viewport_world: egui::Rect,
    placement_gap: f32,
    payload: &ClipboardBoardSelection,
    requested_anchor_world: egui::Pos2,
) -> Result<egui::Pos2, String> {
    let Some(bounds) = clipboard_payload_bounds(payload) else {
        return Ok(requested_anchor_world);
    };
    let existing = canvas
        .committed_placements()
        .into_iter()
        .map(|placement| {
            crate::placement::arrangement::PlacementArrangementInput::new(
                placement.id(),
                placement.state.geometry.center,
                crate::data::rotated_display_size(
                    placement.state.geometry.scaled_size(),
                    placement.state.geometry.rotation,
                ),
            )
        })
        .collect::<Vec<_>>();
    Ok(
        crate::placement::initial_layout::arrange_rect_with_existing_layout(
            &existing,
            requested_anchor_world,
            viewport_world,
            bounds.width(),
            bounds.height(),
            placement_gap,
        ),
    )
}

#[cfg(test)]
mod group_tests {
    use super::*;
    use crate::sources::clipboard::copy_selection_to_json;

    #[test]
    fn group_clipboard_remaps_members_between_boards_and_contains_invalid_metadata() {
        let root = crate::test_support::TempRoot::new("group-clipboard");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let mut source = CanvasState::default();
        let path = library.store_rgba_image("group", &[255; 16], 2, 2).unwrap();
        let first = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0., 0.),
                        1.,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 2, 2, 16, None)
                        .unwrap(),
                ),
            )
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
        let target_board = library.fixture_board("Clipboard destination").unwrap();
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
        malformed.groups[0].members = vec![
            crate::sources::clipboard::ClipboardStackEntry {
                kind: "placement".into(),
                index: 0
            };
            2
        ];
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
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::Pos2::ZERO,
                        1.,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 200, 100, 80000, None)
                        .unwrap(),
                ),
            )
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
                layout: crate::data::GroupLayoutSettings {
                    row_gap: 5.0,
                    ..Default::default()
                },
                gap: 7.,
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
            let mut group = source.groups()[0].clone();
            source.apply_group_layout(&mut group, layout);
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
            assert_eq!(pasted_group.gap, source.groups()[0].gap);
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
    fn exhausted_groups_and_zero_fill_geometry_round_trip_clipboard() {
        let root = crate::test_support::TempRoot::new("group-zero-clipboard");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library
            .store_rgba_image("wide", &vec![255; 20 * 10 * 4], 20, 10)
            .unwrap();
        let asset = library
            .register_original(&path, "image/png", 20, 10, 800, None)
            .unwrap();
        for padding in [50., 70.] {
            for zero in [false, true] {
                let mut source = CanvasState::default();
                let mut member = library
                    .new_placement_from_asset(asset.id, egui::Pos2::ZERO, 1., 0., 0)
                    .unwrap();
                if zero {
                    member.state.geometry.size = egui::Vec2::ZERO;
                }
                let original_size = member.state.geometry.size;
                let id = member.id();
                source.add_unresolved_placement(member);
                let group_id = library.allocate_document_id();
                source.restore_groups(vec![crate::data::Group {
                    id: group_id,
                    name: "Exhausted".into(),
                    members: vec![id],
                    container: RectGeometry::new(
                        egui::Pos2::ZERO,
                        egui::vec2(100., 100.),
                        1.,
                        0.,
                        0,
                    ),
                    flow: crate::data::GroupFlow::Row,
                    gap: 8.,
                    padding,
                    layout: crate::data::GroupLayoutSettings {
                        fixed: [true, true],
                        members: if zero {
                            std::collections::BTreeMap::from([(
                                id,
                                crate::data::MemberLayout {
                                    fill: [true, false],
                                    absolute: false,
                                },
                            )])
                        } else {
                            Default::default()
                        },
                        ..Default::default()
                    },
                }]);
                source.select_group(group_id);
                for loose in [false, true] {
                    if loose {
                        source.ungroup_selected();
                        source.select_placements(&[id]);
                    }
                    let payload = copy_selected_board_payload(&source).unwrap();
                    validate_clipboard_payload(&payload).unwrap();
                    let mut invalid = payload.clone();
                    invalid.placements[0].width = -1.;
                    assert!(validate_clipboard_payload(&invalid).is_err());
                    let serialized = serde_json::to_string(&payload).unwrap();
                    let mut target = CanvasState::default();
                    let mut context = ActiveBoardCommandContext {
                        canvas: &mut target,
                        library: Some(&library),
                        import_worker_available: true,
                        placement_gap: 12.,
                        viewport_world: egui::Rect::from_center_size(
                            egui::Pos2::ZERO,
                            egui::vec2(2000., 2000.),
                        ),
                    };
                    paste_json_at(&mut context, &serialized, egui::pos2(200., 200.))
                        .unwrap()
                        .unwrap();
                    duplicate_selection(&mut context).unwrap().unwrap();
                    let pasted = context.canvas.committed_placements();
                    assert_eq!(pasted.len(), 2);
                    for placement in pasted {
                        assert_ne!(placement.id(), id);
                        assert_eq!(placement.state.geometry.size, original_size);
                        assert_eq!(placement.asset_id(), Some(asset.id));
                    }
                    assert_eq!(context.canvas.groups().len(), if loose { 0 } else { 2 });
                    for group in context.canvas.groups() {
                        assert_eq!(group.padding, padding);
                        assert_eq!(group.container.size, egui::vec2(100., 100.));
                        assert_eq!(group.members.len(), 1);
                        assert_ne!(group.members[0], id);
                        assert_eq!(
                            group
                                .layout
                                .members
                                .get(&group.members[0])
                                .copied()
                                .unwrap_or_default()
                                .fill[0],
                            zero
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn transfer_closes_source_gap_and_keeps_destination_append_order() {
        let root = crate::test_support::TempRoot::new("group-transfer");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library.store_rgba_image("group", &[255; 16], 2, 2).unwrap();
        let first = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0., 0.),
                        1.,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 2, 2, 16, None)
                        .unwrap(),
                ),
            )
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

    #[test]
    fn duplicating_several_members_copies_each_source() {
        let root = crate::test_support::TempRoot::new("group-duplicate-sources");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library
            .store_rgba_image("member", &[255; 16], 2, 2)
            .unwrap();
        let asset = library
            .register_original(&path, "image/png", 2, 2, 16, None)
            .unwrap();
        let mut canvas = CanvasState::default();
        let mut ids = Vec::new();
        for i in 0..3 {
            let p = library
                .new_placement_from_asset(asset.id, egui::pos2(i as f32 * 100., 0.), 1., 0., i)
                .unwrap();
            ids.push(p.id());
            canvas.add_unresolved_placement(p);
        }
        let g = library.allocate_document_id();
        canvas.restore_groups(vec![crate::data::Group {
            id: g,
            name: "Row".into(),
            members: ids.clone(),
            container: RectGeometry::new(egui::Pos2::ZERO, egui::vec2(400., 100.), 1., 0., 0),
            flow: crate::data::GroupFlow::Row,
            gap: 8.,
            padding: 8.,
            layout: crate::data::GroupLayoutSettings {
                members: std::collections::BTreeMap::from([(
                    ids[0],
                    crate::data::MemberLayout {
                        fill: [false, false],
                        absolute: true,
                    },
                )]),
                ..Default::default()
            },
        }]);
        canvas.select_content(&[ids[0], ids[2]], &[]);
        let mut context = ActiveBoardCommandContext {
            canvas: &mut canvas,
            library: Some(&library),
            import_worker_available: true,
            placement_gap: 12.,
            viewport_world: egui::Rect::from_center_size(
                egui::Pos2::ZERO,
                egui::vec2(2000., 2000.),
            ),
        };
        duplicate_selection(&mut context).unwrap().unwrap();
        let group = canvas.groups().iter().find(|group| group.id == g).unwrap();
        let [a, a2, b, c, c2] = group.members[..] else {
            panic!("{:?}", group.members);
        };
        assert_eq!([a, b, c], [ids[0], ids[1], ids[2]]);
        assert!(group.layout.members[&a2].absolute);
        assert!(!group.layout.members.contains_key(&c2));
    }
}

#[cfg(test)]
mod paste_geometry_tests {
    use super::*;

    #[test]
    fn paste_uses_current_geometry_when_moves_and_board_switches_are_not_persisted() {
        let root = crate::test_support::TempRoot::new("paste-current-canvas");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let path = library.store_rgba_image("source", &[255; 4], 1, 1).unwrap();
        let stored = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::Pos2::ZERO,
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&path, "image/png", 100, 100, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let second = library.fixture_board("Second").unwrap();
        // The database still names Board 1 and stores its image at the origin.
        // Each canvas below is the accepted state before queued writes catch up.
        for (board, x) in [(1, 1000.0), (second.id, 2000.0)] {
            let mut canvas = CanvasState::default();
            let mut current = stored.clone();
            if board != 1 {
                current.state.id = library.allocate_document_id();
            }
            current.state.geometry.center = egui::pos2(x, 0.0);
            canvas.add_unresolved_placement(current.clone());
            canvas.select_placements(&[current.id()]);
            let payload = copy_selected_board_payload(&canvas).unwrap();
            let mut context = ActiveBoardCommandContext {
                canvas: &mut canvas,
                library: Some(&library),
                import_worker_available: true,
                placement_gap: 12.0,
                viewport_world: egui::Rect::from_min_max(
                    egui::pos2(-3000.0, -3000.0),
                    egui::pos2(3000.0, 3000.0),
                ),
            };
            paste_board_payload_at(&mut context, &payload, egui::pos2(x, 0.0)).unwrap();
            assert_eq!(
                context
                    .canvas
                    .placement_for_id(context.canvas.selected_placement_ids()[0])
                    .unwrap()
                    .state
                    .geometry
                    .center,
                egui::pos2(x + 112.0, 0.0)
            );
            assert_eq!(library.load_user_session().unwrap().active_board_id, 1);
            assert_eq!(
                library.load_placements().unwrap()[0].state.geometry.center,
                egui::Pos2::ZERO
            );
        }
    }
}
