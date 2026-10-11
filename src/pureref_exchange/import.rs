//! Builds a new board document from a `.pur` file. Runs off the UI thread
//! because it decodes every image and writes originals and crops.

use std::path::Path;

use eframe::egui::{Pos2, Rect, Vec2, pos2, vec2};

use super::counted;
use crate::canvas::{PixelCropRect, textbox_font_size};
use crate::data::{
    Asset, BoardDocumentSnapshot, CanvasAnnotation, CanvasAnnotationColor, CanvasAnnotationKind,
    CanvasStackItem, CanvasStrokeWidth, CanvasTextSize, Placement, PlacementState,
};
use crate::image_preparation::{decode_image_bytes_with_max_side, store_cropped_original};
use crate::library::storage::{Library, sha256_bytes, sha256_file};
use crate::pureref_file::{
    NOTE_FONT_PX, PureRefBoard, PureRefImage, PureRefImageItem, PureRefNote, read_board,
};

pub(crate) struct ImportedPureRefBoard {
    /// Default viewport, and textboxes with no size yet. The app sizes them with
    /// its fonts and frames them with [`Self::image_bounds`] once it knows the screen.
    pub(crate) document: BoardDocumentSnapshot,
    pub(crate) image_bounds: Option<Rect>,
    /// Mirrored items, imported unmirrored because placements have no flip.
    pub(crate) flipped: usize,
    /// Items whose image was linked but missing, or could not be decoded.
    pub(crate) skipped: usize,
}

impl ImportedPureRefBoard {
    pub(crate) fn status(&self) -> String {
        let mut status = format!(
            "Imported {}, {} from {}",
            counted(self.document.placements.len(), "image", "images"),
            counted(self.document.annotations.len(), "note", "notes"),
            self.document.board.name
        );
        if self.flipped > 0 {
            status.push_str(&format!(
                " ({} imported unflipped)",
                counted(self.flipped, "flipped image", "flipped images")
            ));
        }
        if self.skipped > 0 {
            status.push_str(&format!(
                " ({} missing or unreadable)",
                counted(self.skipped, "image", "images")
            ));
        }
        status
    }
}

pub(super) fn import_board(library: &Library, path: &Path) -> Result<ImportedPureRefBoard, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("failed to read file: {error}"))?;
    let board = read_board(&bytes)?;
    let board_name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "PureRef board".to_owned());
    let item_assets = item_assets(library, &board)?;

    let mut placements = Vec::new();
    let mut image_bounds: Option<Rect> = None;
    let mut flipped = 0;
    let mut skipped = 0;
    for (item, asset) in board.image_items.iter().zip(item_assets) {
        let (Some(asset), Some(geometry)) = (asset, ItemGeometry::of(item)) else {
            skipped += 1;
            continue;
        };
        flipped += usize::from(geometry.flipped);
        let state = PlacementState::with_display_size(
            library.allocate_document_id(),
            geometry.center,
            geometry.size,
            geometry.scale,
            geometry.rotation,
            placements.len() as i32,
            false,
        );
        include(&mut image_bounds, state.geometry.visual_bounds());
        placements.push(Placement::loaded(state, asset));
    }
    let annotations: Vec<CanvasAnnotation> = board
        .notes
        .iter()
        .map(|note| note_textbox(library, note))
        .collect();
    // PureRef always draws notes above images.
    let stack_order = placements
        .iter()
        .map(|placement| CanvasStackItem::Placement(placement.state.id))
        .chain(
            annotations
                .iter()
                .map(|annotation| CanvasStackItem::Annotation(annotation.id)),
        )
        .collect();

    Ok(ImportedPureRefBoard {
        document: BoardDocumentSnapshot {
            board: library.new_board(board_name),
            placements,
            annotations,
            groups: Vec::new(),
            stack_order,
            viewport: Default::default(),
        },
        image_bounds,
        flipped,
        skipped,
    })
}

fn include(bounds: &mut Option<Rect>, rect: Rect) {
    *bounds = Some(bounds.map_or(rect, |bounds| bounds.union(rect)));
}

/// The library Asset each image item shows, in item order. Decodes each image
/// once and stores one Asset per distinct crop; the library dedupes repeats.
fn item_assets(library: &Library, board: &PureRefBoard) -> Result<Vec<Option<Asset>>, String> {
    let mut assets = vec![None; board.image_items.len()];
    for (image_index, image) in board.images.iter().enumerate() {
        let items: Vec<usize> = (0..board.image_items.len())
            .filter(|&item| board.image_items[item].image == image_index)
            .collect();
        if items.is_empty() {
            continue;
        }
        let Some(bytes) = image_bytes(image) else {
            continue;
        };
        let Ok(decoded) =
            decode_image_bytes_with_max_side(&bytes, image.source.clone(), None, None)
        else {
            continue;
        };
        let file_name = image
            .source
            .as_deref()
            .and_then(|source| source.rsplit(['/', '\\']).next())
            .filter(|name| !name.is_empty())
            .unwrap_or("pureref-image");
        let mut crop_assets: Vec<(Option<PixelCropRect>, Asset)> = Vec::new();
        for item in items {
            let crop = pixel_crop(&board.image_items[item], decoded.width, decoded.height);
            if let Some((_, asset)) = crop_assets.iter().find(|(stored, _)| *stored == crop) {
                assets[item] = Some(asset.clone());
                continue;
            }
            let asset = match crop {
                None => {
                    let stem = Path::new(file_name)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or(file_name);
                    let path = library.store_original_bytes(stem, &decoded.extension, &bytes)?;
                    library.register_original(
                        &path,
                        &decoded.mime_type,
                        decoded.width,
                        decoded.height,
                        bytes.len() as u64,
                        Some(&sha256_bytes(&bytes)),
                    )?
                }
                Some(rect) => {
                    let (path, cropped) =
                        store_cropped_original(library, &decoded, rect, file_name)?;
                    let file_size = std::fs::metadata(&path)
                        .map_err(|error| format!("failed to read cropped image: {error}"))?
                        .len();
                    library.register_original(
                        &path,
                        "image/png",
                        cropped.width(),
                        cropped.height(),
                        file_size,
                        Some(&sha256_file(&path)?),
                    )?
                }
            };
            assets[item] = Some(asset.clone());
            crop_assets.push((crop, asset));
        }
    }
    Ok(assets)
}

/// Embedded bytes, or the linked file when it still exists on this machine.
fn image_bytes(image: &PureRefImage) -> Option<Vec<u8>> {
    match &image.bytes {
        Some(bytes) => Some(bytes.clone()),
        None => std::fs::read(image.source.as_deref()?).ok(),
    }
}

/// The item's crop in whole pixels, or `None` when it shows the whole image.
fn pixel_crop(item: &PureRefImageItem, width: u32, height: u32) -> Option<PixelCropRect> {
    let clamp = |value: f64, max: u32| value.round().clamp(0.0, f64::from(max)) as u32;
    let crop = item.crop;
    let (left, top) = (clamp(crop.x, width), clamp(crop.y, height));
    let (right, bottom) = (
        clamp(crop.x + crop.width, width),
        clamp(crop.y + crop.height, height),
    );
    if (left, top, right, bottom) == (0, 0, width, height) {
        return None;
    }
    Some(PixelCropRect {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(1),
        height: bottom.saturating_sub(top).max(1),
    })
}

/// A PureRef item transform split into placement geometry.
struct ItemGeometry {
    center: Pos2,
    /// Crop size in image pixels, with the y axis absorbing any non-uniform scale.
    size: Vec2,
    scale: f32,
    rotation: f32,
    flipped: bool,
}

impl ItemGeometry {
    /// `None` for degenerate transforms that collapse the image.
    fn of(item: &PureRefImageItem) -> Option<Self> {
        let transform = item.transform;
        let scale_x = transform.m11.hypot(transform.m12);
        let scale_y = transform.m21.hypot(transform.m22);
        if !(scale_x > 0.0 && scale_y > 0.0 && scale_x.is_finite() && scale_y.is_finite()) {
            return None;
        }
        let flipped = transform.m11 * transform.m22 - transform.m12 * transform.m21 < 0.0;
        // A mirror keeps the image's up direction and drops the horizontal flip.
        let rotation = if flipped {
            (-transform.m21).atan2(transform.m22)
        } else {
            transform.m12.atan2(transform.m11)
        };
        let crop = item.crop;
        let (center_x, center_y) =
            transform.map_point(crop.x + crop.width / 2.0, crop.y + crop.height / 2.0);
        Some(Self {
            center: pos2(center_x as f32, center_y as f32),
            size: vec2(crop.width as f32, (crop.height * scale_y / scale_x) as f32),
            scale: scale_x as f32,
            rotation: rotation as f32,
            flipped,
        })
    }
}

/// A note becomes a textbox centred where PureRef centres it, with the nearest text size.
/// Rotation is dropped. The box starts empty at the note's centre because fonts
/// live on the UI thread.
fn note_textbox(library: &Library, note: &PureRefNote) -> CanvasAnnotation {
    let transform = note.transform;
    let note_font = NOTE_FONT_PX * transform.m11.hypot(transform.m12) as f32;
    let text_size = CanvasTextSize::ALL
        .into_iter()
        .min_by(|a, b| {
            let distance = |size| (textbox_font_size(size) - note_font).abs();
            distance(*a).total_cmp(&distance(*b))
        })
        .unwrap_or_default();
    let (x, y) = transform.map_point(0.0, 0.0);
    let center = pos2(x as f32, y as f32);
    library.new_canvas_annotation(
        CanvasAnnotationKind::Textbox,
        center,
        center,
        note.text.clone(),
        text_size,
        CanvasStrokeWidth::default(),
        CanvasAnnotationColor::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pureref_file::{PureRefPixelRect, PureRefTransform};
    use eframe::egui::emath::Rot2;

    fn item(transform: PureRefTransform, crop: PureRefPixelRect) -> PureRefImageItem {
        PureRefImageItem {
            image: 0,
            name: None,
            transform,
            crop,
            opacity: 1.0,
        }
    }

    #[test]
    fn rotated_scaled_cropped_item_maps_to_crop_center_and_qt_rotation() {
        let angle = 30_f64.to_radians();
        let (sin, cos) = angle.sin_cos();
        let transform = PureRefTransform {
            m11: 2.0 * cos,
            m12: 2.0 * sin,
            m21: -3.0 * sin,
            m22: 3.0 * cos,
            dx: 100.0,
            dy: -50.0,
        };
        let crop = PureRefPixelRect {
            x: 10.0,
            y: 20.0,
            width: 40.0,
            height: 30.0,
        };
        let geometry = ItemGeometry::of(&item(transform, crop)).unwrap();
        // The canvas rotates like `Rot2`, so the placement's top-left corner
        // must land where PureRef draws the crop's top-left corner.
        let half_extent = geometry.size * geometry.scale / 2.0;
        let top_left = geometry.center + Rot2::from_angle(geometry.rotation) * -half_extent;
        let (expected_x, expected_y) = transform.map_point(crop.x, crop.y);

        assert!((top_left - pos2(expected_x as f32, expected_y as f32)).length() < 1e-3);
        assert!((geometry.scale - 2.0).abs() < 1e-5);
        assert!((geometry.size - vec2(40.0, 45.0)).length() < 1e-3);
        assert!(!geometry.flipped);
    }

    #[test]
    fn mirrored_item_keeps_its_up_direction() {
        let transform = PureRefTransform {
            m11: -1.0,
            ..PureRefTransform::IDENTITY
        };
        let crop = PureRefPixelRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let geometry = ItemGeometry::of(&item(transform, crop)).unwrap();

        assert!(geometry.flipped);
        assert_eq!(geometry.rotation, 0.0);
        assert_eq!(geometry.center, pos2(-5.0, 5.0));
    }
}
