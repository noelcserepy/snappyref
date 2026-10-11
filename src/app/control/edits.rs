//! `edit_board`: edits addressed by id, accepted as one Undo step or not at
//! all. The check up front names what is wrong with an edit. The canvas
//! decides whether an edit applies, and a refused one takes the batch back.

use std::collections::{BTreeMap, HashMap};

use eframe::egui::{self, pos2, vec2};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

use super::describe;
use crate::app::SnappyRefApp;
use crate::canvas::CanvasState;
use crate::data::{
    CanvasAnnotationColor, CanvasAnnotationGeometry, CanvasAnnotationKind, CanvasRectangleFill,
    CanvasStrokeWidth, CanvasTextSize, Group, GroupFlow, Placement, WorkspaceSettings,
};
use crate::library::storage::Library;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    edits: Vec<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    action: Action,
    #[serde(rename = "type")]
    kind: Option<Kind>,
    #[serde(rename = "ref")]
    created_ref: Option<String>,
    id: Option<Target>,
    ids: Option<Vec<Target>>,
    group: Option<Target>,
    asset: Option<i64>,
    x: Option<f32>,
    y: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    rotation: Option<f32>,
    dx: Option<f32>,
    dy: Option<f32>,
    x1: Option<f32>,
    y1: Option<f32>,
    x2: Option<f32>,
    y2: Option<f32>,
    text: Option<String>,
    #[serde(default, deserialize_with = "text_size")]
    size: Option<CanvasTextSize>,
    #[serde(default, deserialize_with = "stroke_width")]
    stroke: Option<CanvasStrokeWidth>,
    #[serde(default, deserialize_with = "color")]
    color: Option<CanvasAnnotationColor>,
    #[serde(default, deserialize_with = "fill")]
    fill: Option<CanvasRectangleFill>,
    name: Option<String>,
    #[serde(default, deserialize_with = "flow")]
    flow: Option<GroupFlow>,
    gap: Option<f32>,
    padding: Option<f32>,
    layout: Option<Layout>,
    to: Option<StackEnd>,
}

#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Add,
    Set,
    Delete,
    Group,
    Ungroup,
    AddToGroup,
    RemoveFromGroup,
    Arrange,
    Order,
}

#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Image,
    Label,
    Rectangle,
    Arrow,
    Line,
    /// Made by the group action, never by add.
    #[serde(skip)]
    Group,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Layout {
    Row,
    Column,
    Compact,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum StackEnd {
    Front,
    Back,
}

/// An id, or `"$name"` for what an earlier edit in the call made.
#[derive(Deserialize)]
#[serde(untagged, expecting = "an id number or a \"$name\" ref")]
enum Target {
    Id(i64),
    Ref(String),
}

impl std::fmt::Display for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Id(id) => write!(formatter, "{id}"),
            Self::Ref(name) => write!(formatter, "\"{name}\""),
        }
    }
}

fn word<'de, D: Deserializer<'de>, T>(
    deserializer: D,
    parse: impl Fn(&str) -> Option<T>,
    valid: &str,
) -> Result<Option<T>, D::Error> {
    let word = String::deserialize(deserializer)?;
    parse(&word)
        .map(Some)
        .ok_or_else(|| serde::de::Error::custom(format!("\"{word}\" is not one of {valid}")))
}

fn text_size<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CanvasTextSize>, D::Error> {
    word(d, CanvasTextSize::from_str, "s, m, l, xl, xxl")
}

fn stroke_width<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CanvasStrokeWidth>, D::Error> {
    word(d, CanvasStrokeWidth::from_str, "s, m, l, xl, xxl")
}

fn color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CanvasAnnotationColor>, D::Error> {
    word(
        d,
        CanvasAnnotationColor::from_str,
        "default, coral, gold, mint, sky",
    )
}

fn fill<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CanvasRectangleFill>, D::Error> {
    word(d, CanvasRectangleFill::from_str, "none, tint, solid")
}

fn flow<'de, D: Deserializer<'de>>(d: D) -> Result<Option<GroupFlow>, D::Error> {
    let flows = [
        GroupFlow::Wrap,
        GroupFlow::Row,
        GroupFlow::Column,
        GroupFlow::Free,
    ];
    word(
        d,
        |word| {
            flows
                .into_iter()
                .find(|flow| flow.as_str().to_lowercase() == word)
        },
        "wrap, row, column, free",
    )
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Self::Image => "an image",
            Self::Label => "a label",
            Self::Rectangle => "a rectangle",
            Self::Arrow => "an arrow",
            Self::Line => "a line",
            Self::Group => "a group",
        }
    }

    fn settable(self) -> &'static [&'static str] {
        match self {
            Self::Image => &["x", "y", "dx", "dy", "width", "height", "rotation"],
            Self::Label => &["x", "y", "dx", "dy", "text", "size", "color"],
            Self::Rectangle => &[
                "x", "y", "dx", "dy", "width", "height", "rotation", "color", "fill", "stroke",
            ],
            Self::Arrow | Self::Line => &["x1", "y1", "x2", "y2", "dx", "dy", "color", "stroke"],
            Self::Group => &[
                "x", "y", "dx", "dy", "width", "height", "name", "flow", "gap", "padding",
            ],
        }
    }

    /// Required, then optional.
    fn addable(self) -> Option<(&'static [&'static str], &'static [&'static str])> {
        match self {
            Self::Image => Some((
                &["asset", "x", "y"],
                &["width", "height", "rotation", "group"],
            )),
            Self::Label => Some((&["text", "x", "y"], &["size", "color", "group"])),
            Self::Rectangle => Some((
                &["x", "y", "width", "height"],
                &["rotation", "color", "fill", "stroke", "group"],
            )),
            Self::Arrow | Self::Line => Some((&["x1", "y1", "x2", "y2"], &["color", "stroke"])),
            Self::Group => None,
        }
    }

    fn of(canvas: &CanvasState, id: i64) -> Option<Self> {
        if let Some(annotation) = canvas.annotation_for_id(id) {
            return Some(match annotation.kind {
                CanvasAnnotationKind::Textbox => Self::Label,
                CanvasAnnotationKind::Rectangle => Self::Rectangle,
                CanvasAnnotationKind::Arrow => Self::Arrow,
                CanvasAnnotationKind::Line => Self::Line,
            });
        }
        if canvas.groups().iter().any(|group| group.id == id) {
            return Some(Self::Group);
        }
        canvas.rect_item_geometry(id).map(|_| Self::Image)
    }
}

/// What the check learned that applying needs again.
#[derive(Default)]
struct Checked {
    ref_kinds: HashMap<String, Kind>,
    asset_sizes: HashMap<i64, egui::Vec2>,
}

impl Checked {
    fn kind(&self, canvas: &CanvasState, target: &Target) -> Result<Kind, String> {
        match target {
            Target::Id(id) => {
                Kind::of(canvas, *id).ok_or_else(|| format!("No item or group has id {id}."))
            }
            Target::Ref(name) => name
                .strip_prefix('$')
                .and_then(|name| self.ref_kinds.get(ref_key(name)))
                .copied()
                .ok_or_else(|| {
                    format!(
                        "{target} names nothing. Give an earlier edit \"ref\": \"name\", then \
                         refer to it as \"$name\"."
                    )
                }),
        }
    }

    fn define(&mut self, edit: &Edit, kind: Kind) -> Result<(), String> {
        let Some(name) = &edit.created_ref else {
            return Ok(());
        };
        let name = ref_key(name);
        if name.is_empty() {
            return Err("\"ref\" cannot be empty.".to_owned());
        }
        match self.ref_kinds.insert(name.to_owned(), kind) {
            Some(_) => Err(format!("Two edits use the ref \"{name}\".")),
            None => Ok(()),
        }
    }

    fn check(
        &mut self,
        raw: &Map<String, Value>,
        edit: &Edit,
        canvas: &CanvasState,
        library: &Library,
    ) -> Result<(), String> {
        if let Some((key, _)) = raw.iter().find(|(_, value)| value.is_null()) {
            return Err(format!("\"{key}\" is null. Leave it out instead."));
        }
        numbers(raw)?;
        if edit
            .text
            .as_deref()
            .is_some_and(|text| text.trim().is_empty())
        {
            return Err("\"text\" cannot be empty.".to_owned());
        }
        if edit
            .name
            .as_deref()
            .is_some_and(|name| name.trim().is_empty())
        {
            return Err("\"name\" cannot be empty.".to_owned());
        }
        let targets = edit.id.iter().chain(edit.ids.iter().flatten());
        let kinds = targets
            .map(|target| Ok((target, self.kind(canvas, target)?)))
            .collect::<Result<Vec<_>, String>>()?;
        if edit.ids.as_ref().is_some_and(Vec::is_empty) {
            return Err("\"ids\" is empty.".to_owned());
        }
        let only = |allowed: &[Kind], takes: &str| match kinds
            .iter()
            .find(|(_, kind)| !allowed.contains(kind))
        {
            Some((target, kind)) => Err(format!("{takes}. {target} is {}.", kind.noun())),
            None => Ok(()),
        };
        let members = [Kind::Image, Kind::Label, Kind::Rectangle];
        let in_groups = "A group holds images, labels and rectangles";
        if let Some(group) = &edit.group
            && self.kind(canvas, group)? != Kind::Group
        {
            return Err(format!("{group} is not a group."));
        }
        match edit.action {
            Action::Add => {
                let kind = edit.kind.ok_or(
                    "add needs \"type\". It is one of image, label, rectangle, arrow, line.",
                )?;
                let (required, optional) = kind.addable().ok_or("add cannot make a group.")?;
                fields(
                    raw,
                    &format!("add for {}", kind.noun()),
                    required,
                    &[optional, &["type", "ref"]].concat(),
                )?;
                if let Some(asset) = edit.asset {
                    let found = library.load_assets(&[asset])?.into_values().next();
                    let found = found.ok_or_else(|| {
                        format!("No library asset has id {asset}. Find assets with search_library.")
                    })?;
                    self.asset_sizes
                        .insert(asset, vec2(found.width as f32, found.height as f32));
                    one_image_side(edit)?;
                }
                self.define(edit, kind)
            }
            Action::Set => {
                if edit.id.is_some() == edit.ids.is_some() {
                    return Err("set needs \"id\" or \"ids\", one of them.".to_owned());
                }
                if raw.len() == 2 {
                    return Err("set needs a property to change.".to_owned());
                }
                for (_, kind) in &kinds {
                    fields(
                        raw,
                        &format!("set on {}", kind.noun()),
                        &[],
                        &[kind.settable(), &["id", "ids"]].concat(),
                    )?;
                    if *kind == Kind::Image {
                        one_image_side(edit)?;
                    }
                }
                Ok(())
            }
            Action::Delete => fields(raw, "delete", &["ids"], &[]),
            Action::Group => {
                fields(
                    raw,
                    "group",
                    &["ids"],
                    &["name", "flow", "gap", "padding", "ref"],
                )?;
                only(&members, in_groups)?;
                self.define(edit, Kind::Group)
            }
            Action::Ungroup => {
                fields(raw, "ungroup", &["ids"], &[])?;
                only(&[Kind::Group], "ungroup takes groups")
            }
            Action::AddToGroup => {
                fields(raw, "add_to_group", &["group", "ids"], &[])?;
                only(&members, in_groups)
            }
            Action::RemoveFromGroup => {
                fields(raw, "remove_from_group", &["ids"], &[])?;
                only(&members, in_groups)
            }
            Action::Arrange => {
                fields(raw, "arrange", &["ids", "layout"], &["gap"])?;
                only(&[Kind::Image], "arrange takes images")
            }
            Action::Order => fields(raw, "order", &["ids", "to"], &[]),
        }
    }
}

fn fields(
    raw: &Map<String, Value>,
    subject: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<(), String> {
    let takes = |key: &str| key == "action" || required.contains(&key) || optional.contains(&key);
    if let Some(key) = raw.keys().find(|key| !takes(key)) {
        let valid = [required, optional].concat().join(", ");
        return Err(format!(
            "{subject} does not take \"{key}\". It takes {valid}."
        ));
    }
    match required.iter().find(|key| !raw.contains_key(**key)) {
        Some(key) => Err(format!("{subject} needs \"{key}\".")),
        None => Ok(()),
    }
}

fn numbers(raw: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in raw {
        let Some(number) = value.as_f64() else {
            continue;
        };
        let wrong = match key.as_str() {
            "id" | "group" | "asset" => None,
            _ if number.abs() > 1e7 => Some("is out of range"),
            "width" | "height" if number <= 0.0 => Some("must be above 0"),
            "gap" | "padding" if number < 0.0 => Some("cannot be negative"),
            _ => None,
        };
        if let Some(wrong) = wrong {
            return Err(format!("\"{key}\" {wrong}."));
        }
    }
    Ok(())
}

/// A ref is the same name with or without its `$`.
fn ref_key(name: &str) -> &str {
    name.trim_start_matches('$')
}

/// Grouping refuses with a code. A client needs a sentence.
fn group_error(code: String) -> String {
    match code.as_str() {
        "group_missing" => "The group no longer exists.",
        "item_missing" | "member_missing" => "An item this edit names no longer exists.",
        "invalid_value" => "A group cannot take one of these values.",
        _ => return code,
    }
    .to_owned()
}

fn one_image_side(edit: &Edit) -> Result<(), String> {
    if edit.width.is_some() && edit.height.is_some() {
        return Err("An image keeps its aspect ratio. Give width or height, not both.".to_owned());
    }
    Ok(())
}

/// Applies checked edits to the canvas and remembers what they touched.
struct Batch<'a> {
    ctx: &'a egui::Context,
    canvas: &'a mut CanvasState,
    library: &'a Library,
    settings: &'a WorkspaceSettings,
    image_loading_available: bool,
    asset_sizes: HashMap<i64, egui::Vec2>,
    refs: BTreeMap<String, i64>,
    touched: Vec<i64>,
    deleted: Vec<i64>,
    reload: Vec<Placement>,
}

impl Batch<'_> {
    fn id(&self, target: &Target) -> Result<i64, String> {
        match target {
            Target::Id(id) => Ok(*id),
            Target::Ref(name) => self
                .refs
                .get(ref_key(name))
                .copied()
                .ok_or_else(|| format!("{target} was not created.")),
        }
    }

    fn group(&self, id: i64) -> Option<Group> {
        self.canvas
            .groups()
            .iter()
            .find(|group| group.id == id)
            .cloned()
    }

    /// A Group that loses members lays out the rest again, so it reports too.
    fn leaving_groups(&mut self, ids: &[i64]) {
        let groups = ids
            .iter()
            .filter_map(|id| self.canvas.group_for_member(*id));
        self.touched.extend(groups);
    }

    fn created(&mut self, edit: &Edit, id: i64) {
        self.touched.push(id);
        if let Some(name) = &edit.created_ref {
            self.refs.insert(ref_key(name).to_owned(), id);
        }
    }

    fn apply(&mut self, edit: &Edit) -> Result<(), String> {
        let targets = edit.id.iter().chain(edit.ids.iter().flatten());
        let ids = targets
            .map(|target| self.id(target))
            .collect::<Result<Vec<_>, _>>()?;
        let group = edit
            .group
            .as_ref()
            .map(|group| self.id(group))
            .transpose()?;
        match edit.action {
            Action::Add => {
                let id = self.add(edit)?;
                self.canvas.mark_stack_order_for_persistence();
                if let Some(group) = group {
                    self.canvas
                        .add_members_to_group(group, &[id])
                        .map_err(group_error)?;
                }
                self.created(edit, id);
            }
            Action::Set => {
                for id in ids {
                    self.set(id, edit)?;
                }
            }
            Action::Delete => self.delete(&ids)?,
            Action::Group => {
                let name = edit
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Group {}", self.canvas.groups().len() + 1));
                self.leaving_groups(&ids);
                let id = self
                    .canvas
                    .create_named_group(self.library.allocate_document_id(), name, &ids)
                    .map_err(group_error)?
                    .ok_or("These items cannot form a group.")?;
                self.set_group(id, edit)?;
                self.created(edit, id);
            }
            Action::Ungroup => {
                for id in ids {
                    self.touched
                        .extend(self.group(id).into_iter().flat_map(|group| group.members));
                    self.canvas.dissolve_group(id).map_err(group_error)?;
                    self.deleted.push(id);
                }
            }
            Action::AddToGroup => {
                let group = group.ok_or("add_to_group needs \"group\".")?;
                self.leaving_groups(&ids);
                self.canvas
                    .add_members_to_group(group, &ids)
                    .map_err(group_error)?;
                self.touched.push(group);
            }
            Action::RemoveFromGroup => {
                self.leaving_groups(&ids);
                for id in ids {
                    self.canvas
                        .remove_from_group(id)
                        .map_err(|_| format!("{id} is not in a group."))?;
                    self.touched.push(id);
                }
            }
            Action::Arrange => {
                let gap = edit.gap.unwrap_or(self.settings.placement_gap);
                self.canvas.select_content(&ids, &[]);
                match edit.layout {
                    Some(Layout::Row) => self.canvas.arrange_selected_horizontal(gap),
                    Some(Layout::Column) => self.canvas.arrange_selected_vertical(gap),
                    Some(Layout::Compact) => self.canvas.arrange_selected_compact(gap),
                    None => return Err("arrange needs \"layout\".".to_owned()),
                };
                self.touched.extend(ids);
            }
            Action::Order => {
                let items = ids
                    .iter()
                    .flat_map(|&id| self.group(id).map_or(vec![id], |group| group.members))
                    .collect::<Vec<_>>();
                self.canvas.select_content(&items, &items);
                match edit.to {
                    Some(StackEnd::Front) => self.canvas.bring_selection_to_front(),
                    Some(StackEnd::Back) => self.canvas.send_selection_to_back(),
                    None => return Err("order needs \"to\".".to_owned()),
                };
                self.touched.extend(items);
            }
        }
        Ok(())
    }

    fn add(&mut self, edit: &Edit) -> Result<i64, String> {
        let corner = pos2(edit.x.unwrap_or_default(), edit.y.unwrap_or_default());
        let rotation = edit.rotation.unwrap_or_default().to_radians();
        let kind = match edit.kind {
            Some(Kind::Image) => return self.add_image(edit, corner, rotation),
            Some(Kind::Label) => CanvasAnnotationKind::Textbox,
            Some(Kind::Rectangle) => CanvasAnnotationKind::Rectangle,
            Some(Kind::Arrow) => CanvasAnnotationKind::Arrow,
            Some(Kind::Line) => CanvasAnnotationKind::Line,
            Some(Kind::Group) | None => return Err("add needs \"type\".".to_owned()),
        };
        let text = edit.text.clone().unwrap_or_default();
        let text_size = edit.size.unwrap_or_default();
        let (start, end) = match kind {
            CanvasAnnotationKind::Textbox => (
                corner,
                corner + crate::canvas::textbox_size_for_text(self.ctx, &text, text_size),
            ),
            CanvasAnnotationKind::Rectangle => (
                corner,
                corner
                    + vec2(
                        edit.width.unwrap_or_default(),
                        edit.height.unwrap_or_default(),
                    ),
            ),
            CanvasAnnotationKind::Arrow | CanvasAnnotationKind::Line => (
                pos2(edit.x1.unwrap_or_default(), edit.y1.unwrap_or_default()),
                pos2(edit.x2.unwrap_or_default(), edit.y2.unwrap_or_default()),
            ),
        };
        let mut annotation = self.library.new_canvas_annotation(
            kind,
            start,
            end,
            text,
            text_size,
            edit.stroke.unwrap_or_default(),
            edit.color.unwrap_or_default(),
        );
        annotation.fill = edit.fill.unwrap_or_default();
        if let CanvasAnnotationGeometry::Rect(geometry) = &mut annotation.geometry {
            geometry.rotation = rotation;
        }
        let id = annotation.id;
        self.canvas.add_annotation(annotation);
        self.canvas.mark_annotation_for_persistence(id);
        Ok(id)
    }

    fn add_image(&mut self, edit: &Edit, corner: egui::Pos2, rotation: f32) -> Result<i64, String> {
        let asset = edit.asset.ok_or("add for an image needs \"asset\".")?;
        let natural = *self.asset_sizes.get(&asset).ok_or("Asset unavailable")?;
        let scale = match (edit.width, edit.height) {
            (Some(width), _) => width / natural.x.max(1.0),
            (_, Some(height)) => height / natural.y.max(1.0),
            _ => crate::placement::display_scale_for_placement_target_area(
                natural.x as u32,
                natural.y as u32,
                self.settings.placement_target_area,
            ),
        };
        let front = self
            .canvas
            .committed_placements()
            .iter()
            .map(|placement| placement.state.geometry.z_index)
            .max()
            .map_or(0, |z_index| z_index + 1);
        let placement = self.library.new_placement_from_asset(
            asset,
            corner + natural * scale / 2.0,
            scale,
            rotation,
            front,
        )?;
        let id = placement.state.id;
        // An asset already on this board shares its loaded image.
        if let Err(error) = self
            .canvas
            .add_loaded_reference_placement(placement.clone())
        {
            if !self.image_loading_available {
                return Err(error);
            }
            self.canvas.add_unresolved_placement(placement.clone());
            self.reload.push(placement);
        }
        self.canvas.mark_placement_for_persistence(id);
        Ok(id)
    }

    fn set(&mut self, id: i64, edit: &Edit) -> Result<(), String> {
        if self.group(id).is_some() {
            return self.set_group(id, edit);
        }
        self.canvas.edit_annotation(self.ctx, id, |annotation| {
            if let Some(text) = &edit.text {
                annotation.text = text.clone();
            }
            annotation.text_size = edit.size.unwrap_or(annotation.text_size);
            annotation.stroke_width = edit.stroke.unwrap_or(annotation.stroke_width);
            annotation.color = edit.color.unwrap_or(annotation.color);
            annotation.fill = edit.fill.unwrap_or(annotation.fill);
        });
        let annotation = self.canvas.annotation_for_id(id);
        let offset = vec2(edit.dx.unwrap_or_default(), edit.dy.unwrap_or_default());
        if let Some(CanvasAnnotationGeometry::Segment(segment)) =
            annotation.as_ref().map(|annotation| annotation.geometry)
        {
            let start = pos2(
                edit.x1.unwrap_or(segment.start.x),
                edit.y1.unwrap_or(segment.start.y),
            );
            let end = pos2(
                edit.x2.unwrap_or(segment.end.x),
                edit.y2.unwrap_or(segment.end.y),
            );
            self.canvas
                .set_segment_points(id, start + offset, end + offset);
            self.touched.push(id);
            return Ok(());
        }

        let before = self
            .canvas
            .rect_item_geometry(id)
            .ok_or_else(|| format!("No item or group has id {id}."))?;
        let mut geometry = before;
        let scaled = before.scaled_size();
        if annotation.is_some() {
            if let Some(width) = edit.width {
                geometry.size.x = width / geometry.scale;
            }
            if let Some(height) = edit.height {
                geometry.size.y = height / geometry.scale;
            }
        } else if let Some(width) = edit.width {
            geometry.scale *= width / scaled.x.max(f32::EPSILON);
        } else if let Some(height) = edit.height {
            geometry.scale *= height / scaled.y.max(f32::EPSILON);
        }
        if let Some(rotation) = edit.rotation {
            geometry.rotation = rotation.to_radians();
        }
        // The top-left corner stays put through a resize.
        if positions(edit) {
            let corner = describe::top_left(before);
            let corner = pos2(edit.x.unwrap_or(corner.x), edit.y.unwrap_or(corner.y)) + offset;
            geometry.center = corner + geometry.scaled_size() / 2.0;
        }
        if geometry != before {
            self.canvas.set_rect_item_geometry(id, geometry);
        }
        self.touched.push(id);
        Ok(())
    }

    fn set_group(&mut self, id: i64, edit: &Edit) -> Result<(), String> {
        let missing = || format!("No item or group has id {id}.");
        let before = self.group(id).ok_or_else(missing)?;
        let mut group = before.clone();
        if let Some(name) = &edit.name {
            group.name = name.trim().to_owned();
        }
        group.flow = edit.flow.unwrap_or(group.flow);
        if let Some(gap) = edit.gap {
            group.gap = gap;
            group.layout.row_gap = gap;
        }
        group.padding = edit.padding.unwrap_or(group.padding);
        if let Some(width) = edit.width {
            group.container.size.x = width / group.container.scale;
        }
        if let Some(height) = edit.height {
            group.container.size.y = height / group.container.scale;
        }
        if group != before {
            self.canvas.edit_group(group).map_err(group_error)?;
        }
        if positions(edit) {
            let corner = describe::top_left(before.container);
            let corner = pos2(edit.x.unwrap_or(corner.x), edit.y.unwrap_or(corner.y))
                + vec2(edit.dx.unwrap_or_default(), edit.dy.unwrap_or_default());
            let laid_out = self.group(id).ok_or_else(missing)?;
            let delta = corner - describe::top_left(laid_out.container);
            if delta != egui::Vec2::ZERO {
                self.canvas.move_group(id, delta);
            }
        }
        self.touched.push(id);
        Ok(())
    }

    fn delete(&mut self, ids: &[i64]) -> Result<(), String> {
        let mut items = Vec::new();
        let mut groups = Vec::new();
        for &id in ids {
            match self.group(id) {
                Some(group) => {
                    items.extend(group.members);
                    groups.push(id);
                }
                None => items.push(id),
            }
        }
        self.leaving_groups(&items);
        self.canvas.delete_items(&items);
        for &id in &groups {
            if self.group(id).is_some() {
                self.canvas.dissolve_group(id).map_err(group_error)?;
            }
        }
        self.deleted.extend(items);
        self.deleted.extend(groups);
        Ok(())
    }
}

/// Whether the edit moves or resizes, which both place the top-left corner.
fn positions(edit: &Edit) -> bool {
    [edit.x, edit.y, edit.dx, edit.dy, edit.width, edit.height]
        .iter()
        .any(Option::is_some)
}

impl SnappyRefApp {
    pub(super) fn control_edit_board(
        &mut self,
        ctx: &egui::Context,
        arguments: Value,
    ) -> Result<Value, String> {
        let Arguments { edits: raw } = super::arguments(arguments)?;
        if raw.is_empty() {
            return Err("\"edits\" is empty.".to_owned());
        }
        if self.canvas.user_edit_in_progress() {
            return Err(
                "The user is in the middle of an edit on the board. Try again in a moment."
                    .to_owned(),
            );
        }
        let library = self.library.as_ref().ok_or("Storage unavailable")?;
        let mut checked = Checked::default();
        let mut edits = Vec::with_capacity(raw.len());
        for (index, raw) in raw.iter().enumerate() {
            let rejected = |error: String| format!("edits[{index}]: {error} Nothing was applied.");
            let edit: Edit = serde_json::from_value(Value::Object(raw.clone()))
                .map_err(|error| rejected(format!("{error}.")))?;
            checked
                .check(raw, &edit, &self.canvas, library)
                .map_err(rejected)?;
            edits.push(edit);
        }

        // What the user changed and the app has not recorded yet stays their own Undo step.
        self.record_pending_document_changes_history("Edit board");
        let library = self.library.as_ref().ok_or("Storage unavailable")?;
        let (settings, image_loading_available) = (&self.settings, self.import_worker.is_some());
        // arrange and order borrow the selection.
        let selection = self.canvas.selection();
        let mut batch = Batch {
            ctx,
            canvas: &mut self.canvas,
            library,
            settings,
            image_loading_available,
            asset_sizes: checked.asset_sizes,
            refs: BTreeMap::new(),
            touched: Vec::new(),
            deleted: Vec::new(),
            reload: Vec::new(),
        };
        let failure = edits
            .iter()
            .enumerate()
            .find_map(|(index, edit)| Some((index, batch.apply(edit).err()?)));
        let (refs, touched, deleted, reload) =
            (batch.refs, batch.touched, batch.deleted, batch.reload);
        if let Some((index, error)) = failure {
            let discarded = self.board_lifecycle().discard_direct_edits(ctx);
            self.canvas.restore_selection(selection);
            discarded?;
            return Err(format!(
                "edits[{index}] failed: {error} Nothing was applied."
            ));
        }
        self.canvas.restore_selection(selection);

        let viewport = self.import_viewport_world_rect(ctx.viewport_rect());
        let (owner, mut context) = self.board_edit_context(viewport);
        owner.accept_direct_edits(&mut context, "AI edit", reload)?;

        // A Group lays its members out again, so every member of a touched Group reports.
        let groups = self
            .canvas
            .groups()
            .iter()
            .filter(|group| {
                touched.contains(&group.id) || group.members.iter().any(|id| touched.contains(id))
            })
            .collect::<Vec<_>>();
        let mut items = Vec::new();
        for id in touched
            .iter()
            .chain(groups.iter().flat_map(|group| &group.members))
        {
            if !items.contains(id) {
                items.push(*id);
            }
        }
        let mut result = Map::new();
        let items = items
            .into_iter()
            .filter_map(|id| describe::item(&self.canvas, id))
            .collect::<Vec<_>>();
        result.insert("items".into(), json!(items));
        if !groups.is_empty() {
            let groups = groups.into_iter().map(describe::group).collect::<Vec<_>>();
            result.insert("groups".into(), json!(groups));
        }
        if !refs.is_empty() {
            result.insert("refs".into(), json!(refs));
        }
        if !deleted.is_empty() {
            result.insert("deleted".into(), json!(deleted));
        }
        Ok(Value::Object(result))
    }
}
