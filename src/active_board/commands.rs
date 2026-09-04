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
    width: u32,
    height: u32,
}

impl ClipboardPlacementState {
    fn geometry(&self) -> RectGeometry {
        RectGeometry::new(
            egui::pos2(self.center_x, self.center_y),
            egui::vec2(self.width as f32, self.height as f32),
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
    let item_count = payload.placements.len() + payload.annotations.len();
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
            width: asset.width,
            height: asset.height,
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

    (!placements.is_empty() || !annotations.is_empty()).then(|| ClipboardBoardSelection {
        kind: COPY_PAYLOAD_KIND.to_owned(),
        source_center_x: source_bounds.center().x,
        source_center_y: source_bounds.center().y,
        placements,
        annotations,
        stack_order,
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
        || (payload.placements.is_empty() && payload.annotations.is_empty())
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
        let placement = library.new_placement_from_asset(
            placement.asset_id,
            geometry.center,
            geometry.scale,
            geometry.rotation,
            geometry.z_index,
        )?;
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

    context
        .canvas
        .select_content(&pasted_placement_ids, &pasted_annotation_ids);
    move_pasted_content_to_front(
        context.canvas,
        &pasted_placement_ids,
        &pasted_annotation_ids,
        payload,
    );
    let count = pasted_placement_ids.len() + pasted_annotation_ids.len();
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
