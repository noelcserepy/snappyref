mod read;
pub(crate) use read::{ClipboardContent, read};

#[cfg(target_os = "macos")]
pub(crate) mod macos;
#[cfg(target_os = "windows")]
pub(crate) mod windows;
#[cfg(target_os = "macos")]
use macos as native;
#[cfg(target_os = "windows")]
use windows as native;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod native {
    pub(super) fn clipboard_paths() -> Vec<std::path::PathBuf> {
        Vec::new()
    }
    pub(super) fn clipboard_pureref_binary() -> Option<Vec<u8>> {
        None
    }
}

use std::borrow::Cow;

use crate::canvas::CanvasState;
use crate::data::{
    CanvasAnnotationGeometry, CanvasAnnotationKind, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, RectGeometry,
};
use crate::library::storage::Library;
use eframe::egui;
use serde::{Deserialize, Serialize};

const MAX_EXTERNAL_CLIPBOARD_IMAGE_BYTES: usize = 200 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardCopyPayload {
    pub(crate) internal_json: String,
    pub(crate) item_count: usize,
    pub(crate) external_image: Option<ExternalClipboardImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExternalClipboardImage {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn build_selection_copy_payload(
    canvas: &CanvasState,
    library: Option<&Library>,
) -> Result<Option<ClipboardCopyPayload>, String> {
    let Some(outcome) = copy_selection_to_json(canvas)? else {
        return Ok(None);
    };
    let external_image = external_image_for_selection(canvas, library)?;
    Ok(Some(ClipboardCopyPayload {
        internal_json: outcome.payload_json,
        item_count: outcome.item_count,
        external_image,
    }))
}

fn external_image_for_selection(
    canvas: &CanvasState,
    library: Option<&Library>,
) -> Result<Option<ExternalClipboardImage>, String> {
    let selected_placement_ids = canvas.selected_placement_ids();
    if selected_placement_ids.len() != 1 || !canvas.selected_annotation_ids().is_empty() {
        return Ok(None);
    }

    let Some(library) = library else {
        return Ok(None);
    };
    let placement = canvas
        .placement_for_id(selected_placement_ids[0])
        .ok_or_else(|| "missing selected placement".to_owned())?;
    let Ok(byte_len) = external_image_byte_len(
        placement.require_loaded_asset().width,
        placement.require_loaded_asset().height,
    ) else {
        return Ok(None);
    };
    if byte_len > MAX_EXTERNAL_CLIPBOARD_IMAGE_BYTES {
        return Ok(None);
    }

    let Ok((rgba, width, height)) = crate::image_preparation::load_canvas_snapshot_pixels(
        library,
        &placement,
        placement.require_loaded_asset().width,
        placement.require_loaded_asset().height,
    ) else {
        return Ok(None);
    };
    Ok(Some(ExternalClipboardImage {
        rgba,
        width,
        height,
    }))
}

fn external_image_byte_len(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "external clipboard image dimensions overflowed".to_owned())
}

pub(crate) fn write_payload(payload: &ClipboardCopyPayload) -> Result<(), String> {
    match &payload.external_image {
        Some(image) => write_image_payload_with_text(image, &payload.internal_json),
        None => {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|error| format!("Clipboard unavailable: {error}"))?;
            clipboard
                .set_text(payload.internal_json.clone())
                .map_err(|error| format!("Failed to copy selection: {error}"))
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn write_image_payload_with_text(image: &ExternalClipboardImage, text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?;
    clipboard
        .set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: Cow::Borrowed(&image.rgba),
        })
        .map_err(|error| format!("Failed to copy image: {error}"))?;
    native::append_internal_text(text)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_image_payload_with_text(
    _image: &ExternalClipboardImage,
    text: &str,
) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?;
    clipboard
        .set_text(text.to_owned())
        .map_err(|error| format!("Failed to copy selection: {error}"))
}

#[cfg(test)]
mod tests {
    use super::build_selection_copy_payload;
    use crate::data::{Asset, Placement, PlacementState};
    use crate::test_support::TempRoot;

    #[test]
    fn single_selected_placement_copies_internal_json_and_external_image() {
        let ctx = eframe::egui::Context::default();
        let root = TempRoot::new("snappyref-copy-external-image");
        let library = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        let source = library
            .store_rgba_image("copy-source", &rgba, 2, 1)
            .unwrap();
        let placement = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        eframe::egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(
                            &source,
                            "image/png",
                            2,
                            1,
                            std::fs::metadata(&source).unwrap().len(),
                            None,
                        )
                        .unwrap(),
                ),
            )
            .unwrap();
        let placement_id = placement.state.id;
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(&ctx, loaded_item(placement, rgba.clone(), 2, 1));
        canvas.select_placements(&[placement_id]);

        let payload = build_selection_copy_payload(&canvas, Some(&library))
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 1);
        assert!(
            payload
                .internal_json
                .contains(crate::sources::clipboard::COPY_PAYLOAD_KIND)
        );
        assert_eq!(
            payload.external_image,
            Some(super::ExternalClipboardImage {
                rgba,
                width: 2,
                height: 1,
            })
        );
    }

    #[test]
    fn mixed_selection_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(
            &ctx,
            loaded_item(test_record(1, 1), vec![255, 255, 255, 255], 1, 1),
        );
        canvas.add_annotation(crate::data::CanvasAnnotation::new(
            20,
            crate::data::CanvasAnnotationKind::Textbox,
            eframe::egui::pos2(10.0, 10.0),
            eframe::egui::pos2(70.0, 40.0),
            "caption".to_owned(),
            crate::data::CanvasTextSize::M,
            crate::data::CanvasStrokeWidth::M,
            crate::data::CanvasAnnotationColor::Default,
            false,
            false,
        ));
        canvas.select_content(&[1], &[20]);

        let payload = build_selection_copy_payload(&canvas, None)
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 2);
        assert!(payload.external_image.is_none());
    }

    #[test]
    fn oversized_single_placement_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let root = TempRoot::new("snappyref-copy-oversized-image");
        let library = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
        for side in [20_000, u32::MAX] {
            let mut canvas = crate::canvas::CanvasState::default();
            let mut placement = test_record(1, 1);
            placement.require_loaded_asset_mut().width = side;
            placement.require_loaded_asset_mut().height = side;
            placement.state.geometry.size = eframe::egui::vec2(side as f32, side as f32);
            canvas.add_prepared_placement_image(
                &ctx,
                loaded_item(placement, vec![255, 255, 255, 255], 1, 1),
            );
            canvas.select_placements(&[1]);

            let payload = build_selection_copy_payload(&canvas, Some(&library))
                .unwrap()
                .unwrap();

            assert_eq!(payload.item_count, 1);
            assert!(payload.external_image.is_none());
            assert!(
                super::decode_board_selection(&payload.internal_json)
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[test]
    fn unsupported_single_placement_copies_metadata_only() {
        let ctx = eframe::egui::Context::default();
        let root = TempRoot::new("snappyref-copy-unsupported-image");
        let library = crate::library::storage::Library::initialize(root.to_path_buf()).unwrap();
        let mut placement = test_record(1, 1);
        placement.require_loaded_asset_mut().mime_type = "image/svg+xml".to_owned();

        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_prepared_placement_image(
            &ctx,
            loaded_item(placement, vec![255, 255, 255, 255], 1, 1),
        );
        canvas.select_placements(&[1]);

        let payload = build_selection_copy_payload(&canvas, Some(&library))
            .unwrap()
            .unwrap();

        assert_eq!(payload.item_count, 1);
        assert!(payload.external_image.is_none());
    }

    fn loaded_item(
        placement: Placement,
        preview_rgba: Vec<u8>,
        preview_width: u32,
        preview_height: u32,
    ) -> crate::image_preparation::PreparedPlacementImage {
        crate::image_preparation::PreparedPlacementImage {
            target_board_id: 1,
            queue_id: None,
            placement,
            preview_rgba,
            preview_width,
            preview_height,
            animated_preview: None,
            preview_texture: crate::image_preparation::ImagePrepPreviewTexture::Cpu,
            loaded_lod: crate::data::CanvasImageLod::Working,
            existing_load_diagnostics: None,
            origin: crate::image_preparation::ImportOrigin::Existing,
            trace: None,
        }
    }

    fn test_record(placement_id: i64, asset_id: i64) -> Placement {
        Placement::loaded(
            PlacementState {
                id: placement_id,
                geometry: crate::data::RectGeometry::new(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::Vec2::ZERO,
                    1.0,
                    0.0,
                    0,
                ),
                archived: false,
            },
            Asset {
                id: asset_id,
                original_path: std::path::PathBuf::from(format!("fixture-{asset_id}.png")),
                filename: format!("fixture-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width: 1,
                height: 1,
            },
        )
    }
}

// The clipboard kind string is compatibility data. The typed payload below
// distinguishes Placements from Annotations even though old JSON says "items".
pub(crate) const COPY_PAYLOAD_KIND: &str = "snappyref-board-items";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopyCommandOutcome {
    pub(crate) payload_json: String,
    pub(crate) item_count: usize,
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
    pub(crate) name: String,
    pub(crate) members: Vec<usize>,
    pub(crate) container: [f32; 4],
    pub(crate) flow: crate::data::GroupFlow,
    pub(crate) spacing: f32,
    pub(crate) padding: f32,
}
impl ClipboardGroup {
    pub(crate) fn bounds(&self) -> egui::Rect {
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
    pub(crate) asset_id: i64,
    pub(crate) center_x: f32,
    pub(crate) center_y: f32,
    pub(crate) scale: f32,
    pub(crate) rotation: f32,
    pub(crate) z_index: i32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl ClipboardPlacementState {
    pub(crate) fn geometry(&self) -> RectGeometry {
        RectGeometry::new(
            egui::pos2(self.center_x, self.center_y),
            egui::vec2(self.width, self.height),
            self.scale,
            self.rotation,
            self.z_index,
        )
    }

    pub(crate) fn translated_geometry(&self, delta: egui::Vec2) -> RectGeometry {
        let mut geometry = self.geometry();
        geometry.center += delta;
        geometry
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ClipboardAnnotation {
    pub(crate) kind: String,
    pub(crate) start_x: f32,
    pub(crate) start_y: f32,
    pub(crate) end_x: f32,
    pub(crate) end_y: f32,
    pub(crate) text: String,
    pub(crate) text_size: String,
    pub(crate) stroke_width: String,
    pub(crate) color: String,
    pub(crate) background_enabled: bool,
}

impl ClipboardAnnotation {
    pub(crate) fn geometry(&self) -> Result<CanvasAnnotationGeometry, String> {
        let kind = CanvasAnnotationKind::from_str(&self.kind)
            .ok_or_else(|| format!("unknown copied annotation kind: {}", self.kind))?;
        Ok(CanvasAnnotationGeometry::from_kind(
            kind,
            egui::pos2(self.start_x, self.start_y),
            egui::pos2(self.end_x, self.end_y),
        ))
    }

    pub(crate) fn translated_geometry(
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

pub(crate) fn validate_clipboard_payload(payload: &ClipboardBoardSelection) -> Result<(), String> {
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

pub(crate) fn decode_board_selection(
    text: &str,
) -> Result<Option<ClipboardBoardSelection>, String> {
    let Ok(payload) = serde_json::from_str::<ClipboardBoardSelection>(text) else {
        return Ok(None);
    };
    if payload.kind != COPY_PAYLOAD_KIND
        || (payload.placements.is_empty()
            && payload.annotations.is_empty()
            && payload.groups.is_empty())
    {
        return Ok(None);
    }
    validate_clipboard_payload(&payload)?;
    Ok(Some(payload))
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests;
