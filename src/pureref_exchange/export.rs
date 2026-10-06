//! Writes the active board to a `.pur` file. The plan is a cheap snapshot of
//! the canvas taken on the UI thread; reading originals and writing the file
//! run off it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::Rect;

use super::counted;
use crate::canvas::{CanvasState, textbox_font_size};
use crate::data::{Asset, CanvasAnnotationKind, CanvasStackItem, RectGeometry};
use crate::pureref_file::{
    NOTE_FONT_PX, PureRefBoard, PureRefImage, PureRefImageItem, PureRefNote, PureRefPixelRect,
    PureRefTransform, write_board,
};

/// Formats PureRef's Qt build decodes. Anything else is left out of the file.
const PUREREF_IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/bmp",
    "image/webp",
    "image/tiff",
];

pub(crate) struct PureRefExportPlan {
    /// Loaded placements, back to front.
    placements: Vec<(Asset, RectGeometry)>,
    notes: Vec<PureRefNote>,
    /// Lines and arrows, which PureRef has no item for.
    skipped_annotations: usize,
}

impl PureRefExportPlan {
    pub(crate) fn from_canvas(canvas: &CanvasState) -> Self {
        let placements: HashMap<i64, _> = canvas
            .committed_placements()
            .into_iter()
            .filter_map(|placement| {
                let asset = placement.loaded_asset()?.clone();
                Some((placement.state.id, (asset, placement.state.geometry)))
            })
            .collect();
        let annotations: HashMap<i64, _> = canvas
            .annotations()
            .into_iter()
            .filter(|annotation| !annotation.archived)
            .map(|annotation| (annotation.id, annotation))
            .collect();
        let mut plan = Self {
            placements: Vec::new(),
            notes: Vec::new(),
            skipped_annotations: 0,
        };
        for item in canvas.canvas_stack_order() {
            match item {
                CanvasStackItem::Placement(id) => {
                    plan.placements.extend(placements.get(&id).cloned());
                }
                CanvasStackItem::Annotation(id) => {
                    let Some(annotation) = annotations.get(&id) else {
                        continue;
                    };
                    if annotation.kind != CanvasAnnotationKind::Textbox {
                        plan.skipped_annotations += 1;
                        continue;
                    }
                    let scale = f64::from(textbox_font_size(annotation.text_size) / NOTE_FONT_PX);
                    let center = Rect::from_two_pos(annotation.start(), annotation.end()).center();
                    plan.notes.push(PureRefNote {
                        text: annotation.text.clone(),
                        transform: PureRefTransform {
                            m11: scale,
                            m22: scale,
                            ..PureRefTransform::translation(
                                f64::from(center.x),
                                f64::from(center.y),
                            )
                        },
                    });
                }
            }
        }
        plan
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.placements.is_empty() && self.notes.is_empty()
    }
}

pub(crate) struct ExportedPureRefBoard {
    pub(crate) path: PathBuf,
    pub(crate) images: usize,
    pub(crate) notes: usize,
    /// Images in formats PureRef can't open, or whose original is gone.
    pub(crate) skipped_images: usize,
    pub(crate) skipped_annotations: usize,
}

impl ExportedPureRefBoard {
    pub(crate) fn status(&self) -> String {
        let file_name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let mut status = format!(
            "Exported {}, {} to {file_name}",
            counted(self.images, "image", "images"),
            counted(self.notes, "note", "notes")
        );
        if self.skipped_images > 0 {
            status.push_str(&format!(
                " ({} in unsupported formats left out)",
                counted(self.skipped_images, "image", "images")
            ));
        }
        if self.skipped_annotations > 0 {
            status.push_str(&format!(
                " ({} left out)",
                counted(
                    self.skipped_annotations,
                    "line or arrow",
                    "lines and arrows"
                )
            ));
        }
        status
    }
}

pub(super) fn export_board(
    plan: PureRefExportPlan,
    path: PathBuf,
) -> Result<ExportedPureRefBoard, String> {
    let mut board = PureRefBoard {
        images: Vec::new(),
        image_items: Vec::new(),
        notes: plan.notes,
    };
    // Each asset is read once; `None` marks one PureRef can't show.
    let mut image_indices: HashMap<i64, Option<usize>> = HashMap::new();
    let mut skipped_images = 0;
    for (asset, geometry) in &plan.placements {
        let image = *image_indices.entry(asset.id).or_insert_with(|| {
            let bytes = PUREREF_IMAGE_MIME_TYPES
                .contains(&asset.mime_type.as_str())
                .then(|| std::fs::read(&asset.original_path).ok())
                .flatten()?;
            board.images.push(PureRefImage {
                bytes: Some(bytes),
                source: Some(asset.original_path.to_string_lossy().into_owned()),
                width: asset.width,
                height: asset.height,
            });
            Some(board.images.len() - 1)
        });
        let Some(image) = image else {
            skipped_images += 1;
            continue;
        };
        board.image_items.push(PureRefImageItem {
            image,
            name: None,
            transform: item_transform(*geometry, asset.width, asset.height),
            crop: PureRefPixelRect {
                x: 0.0,
                y: 0.0,
                width: f64::from(asset.width),
                height: f64::from(asset.height),
            },
            opacity: 1.0,
        });
    }

    write_atomically(&path, &write_board(&board)?)?;
    Ok(ExportedPureRefBoard {
        path,
        images: board.image_items.len(),
        notes: board.notes.len(),
        skipped_images,
        skipped_annotations: plan.skipped_annotations,
    })
}

/// Maps image pixels (top-left origin) onto the placement's rotated, scaled rect.
fn item_transform(geometry: RectGeometry, width: u32, height: u32) -> PureRefTransform {
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let scaled_size = geometry.scaled_size();
    let scale_x = f64::from(scaled_size.x) / width;
    let scale_y = f64::from(scaled_size.y) / height;
    let (sin, cos) = f64::from(geometry.rotation).sin_cos();
    let (m11, m12, m21, m22) = (scale_x * cos, scale_x * sin, -scale_y * sin, scale_y * cos);
    PureRefTransform {
        m11,
        m12,
        m21,
        m22,
        dx: f64::from(geometry.center.x) - (m11 * width + m21 * height) / 2.0,
        dy: f64::from(geometry.center.y) - (m12 * width + m22 * height) / 2.0,
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("failed to write {}: {error}", path.display())
        })
}
