//! The JSON an AI client reads back: items, Groups and boards in the
//! vocabulary of the tool catalogue.

use eframe::egui;
use serde_json::{Map, Value, json};

use crate::canvas::CanvasState;
use crate::data::{
    CanvasAnnotation, CanvasAnnotationGeometry, CanvasAnnotationKind, CanvasStackItem, Group,
    GroupId, Placement, PlacementContent, RectGeometry,
};

/// A tenth of a world unit is finer than any edit an AI client makes, and
/// keeps board listings short.
pub(super) fn round(value: f32) -> Value {
    json!((f64::from(value) * 10.0).round() / 10.0)
}

pub(super) fn top_left(geometry: RectGeometry) -> egui::Pos2 {
    geometry.center - geometry.scaled_size() / 2.0
}

fn put_rect(item: &mut Map<String, Value>, geometry: RectGeometry) {
    let (min, size) = (top_left(geometry), geometry.scaled_size());
    item.insert("x".into(), round(min.x));
    item.insert("y".into(), round(min.y));
    item.insert("width".into(), round(size.x));
    item.insert("height".into(), round(size.y));
    if geometry.rotation != 0.0 {
        item.insert("rotation".into(), round(geometry.rotation.to_degrees()));
    }
}

fn image(placement: &Placement, group: Option<GroupId>) -> Value {
    let mut item = Map::new();
    item.insert("id".into(), json!(placement.state.id));
    item.insert("type".into(), json!("image"));
    put_rect(&mut item, placement.state.geometry);
    if let Some(asset) = placement.asset_id() {
        item.insert("asset".into(), json!(asset));
    }
    match &placement.content {
        PlacementContent::Loaded { asset } => {
            item.insert("file".into(), json!(asset.filename));
        }
        PlacementContent::Loading { .. } => {
            item.insert("status".into(), json!("loading"));
        }
        PlacementContent::Failed { message, .. } => {
            item.insert("status".into(), json!("failed"));
            item.insert("error".into(), json!(message));
        }
    }
    if let Some(group) = group {
        item.insert("group".into(), json!(group));
    }
    Value::Object(item)
}

fn annotation(annotation: &CanvasAnnotation, group: Option<GroupId>) -> Value {
    let mut item = Map::new();
    item.insert("id".into(), json!(annotation.id));
    let kind = match annotation.kind {
        CanvasAnnotationKind::Textbox => "label",
        kind => kind.as_str(),
    };
    item.insert("type".into(), json!(kind));
    match annotation.geometry {
        CanvasAnnotationGeometry::Rect(geometry) => put_rect(&mut item, geometry),
        CanvasAnnotationGeometry::Segment(segment) => {
            item.insert("x1".into(), round(segment.start.x));
            item.insert("y1".into(), round(segment.start.y));
            item.insert("x2".into(), round(segment.end.x));
            item.insert("y2".into(), round(segment.end.y));
        }
    }
    if annotation.kind == CanvasAnnotationKind::Textbox {
        item.insert("text".into(), json!(annotation.text));
        item.insert("size".into(), json!(annotation.text_size.as_str()));
    } else {
        item.insert("stroke".into(), json!(annotation.stroke_width.as_str()));
    }
    item.insert("color".into(), json!(annotation.color.as_str()));
    if annotation.kind == CanvasAnnotationKind::Rectangle {
        item.insert("fill".into(), json!(annotation.fill.as_str()));
    }
    if let Some(group) = group {
        item.insert("group".into(), json!(group));
    }
    Value::Object(item)
}

pub(super) fn group(group: &Group) -> Value {
    let mut described = Map::new();
    described.insert("id".into(), json!(group.id));
    described.insert("name".into(), json!(group.name));
    put_rect(&mut described, group.container);
    described.insert("flow".into(), json!(group.flow.as_str().to_lowercase()));
    described.insert("gap".into(), round(group.gap));
    described.insert("padding".into(), round(group.padding));
    described.insert("members".into(), json!(group.members));
    Value::Object(described)
}

/// An item on the open board, or None once it is gone.
pub(super) fn item(canvas: &CanvasState, id: i64) -> Option<Value> {
    let group = canvas.group_for_member(id);
    canvas
        .placement_for_id(id)
        .map(|placement| image(&placement, group))
        .or_else(|| {
            canvas
                .annotation_for_id(id)
                .map(|found| annotation(&found, group))
        })
}

/// Items back to front, then Groups.
pub(super) fn document(
    placements: &[Placement],
    annotations: &[CanvasAnnotation],
    groups: &[Group],
    stack_order: &[CanvasStackItem],
) -> (Vec<Value>, Vec<Value>) {
    let group_of = |id: i64| {
        groups
            .iter()
            .find(|group| group.members.contains(&id))
            .map(|group| group.id)
    };
    let items = stack_order
        .iter()
        .filter_map(|entry| match *entry {
            CanvasStackItem::Placement(id) => placements
                .iter()
                .find(|placement| placement.state.id == id && !placement.state.archived)
                .map(|placement| image(placement, group_of(id))),
            CanvasStackItem::Annotation(id) => annotations
                .iter()
                .find(|found| found.id == id && !found.archived)
                .map(|found| annotation(found, group_of(id))),
        })
        .collect();
    (items, groups.iter().map(group).collect())
}
