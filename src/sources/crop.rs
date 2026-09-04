#[derive(Clone, Debug)]
pub(crate) struct CroppedImageIntegration {
    pub(crate) updated_record: crate::data::Placement,
    pub(crate) previous_record: crate::data::Placement,
    pub(crate) indexed_asset: Option<(i64, std::path::PathBuf)>,
}

pub(crate) fn enqueue_pending_requests(
    ctx: &eframe::egui::Context,
    canvas: &mut crate::canvas::CanvasState,
    library: Option<&crate::library::storage::Library>,
    import_worker: Option<&crate::jobs::ImagePrepWorker>,
    max_texture_side: usize,
    canvas_image_max_side: u32,
) -> Result<bool, String> {
    let Some(import_worker) = import_worker else {
        return Ok(false);
    };
    let Some(library) = library else {
        return Err("Storage unavailable".to_owned());
    };

    let mut enqueued = false;
    while let Some(request) = canvas.take_pending_crop() {
        let placement_id = request.placement_id;
        canvas.mark_crop_pending(placement_id);
        if let Err(error) = canvas.warm_cached_pixels_for_item(library, placement_id) {
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
        if let Err(error) = canvas.apply_pending_crop_preview(ctx, &request) {
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
        if let Err(error) = import_worker.enqueue(crate::jobs::ImagePrepRequest::Crop {
            request,
            max_texture_side,
            canvas_image_settings: crate::jobs::CanvasImageSettings {
                max_side: canvas_image_max_side,
            },
        }) {
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
        enqueued = true;
    }

    Ok(enqueued)
}

pub(crate) fn integrate_cropped_result(
    ctx: &eframe::egui::Context,
    canvas: &mut crate::canvas::CanvasState,
    library: Option<&crate::library::storage::Library>,
    cropped: crate::jobs::PreparedCropPreview,
    semantic_search_enabled: bool,
) -> Result<CroppedImageIntegration, String> {
    let placement_id = cropped.placement_id;
    let stored_path = cropped.stored_path.clone();
    let previous_snapshot = match canvas.snapshot_item_image(placement_id) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            cleanup_failed_crop(&stored_path);
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
    };

    let Some(library) = library else {
        cleanup_failed_crop(&stored_path);
        canvas.clear_crop_pending(placement_id);
        return Err("Storage unavailable".to_owned());
    };

    let mut updated_item = match canvas.projected_cropped_item(&cropped) {
        Ok(item) => item,
        Err(error) => {
            cleanup_failed_crop(&stored_path);
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
    };

    let asset = match library.create_asset(
        &stored_path,
        image::ImageFormat::Png.to_mime_type(),
        cropped.width,
        cropped.height,
        cropped.file_size,
    ) {
        Ok(asset) => asset,
        Err(error) => {
            cleanup_failed_crop(&stored_path);
            canvas.clear_crop_pending(placement_id);
            return Err(error);
        }
    };

    updated_item.asset_id = Some(asset.id);

    let updated_item_id = updated_item.id;
    let updated_record = crate::data::Placement::loaded(updated_item.clone(), asset.clone());

    match canvas.apply_cropped_item(ctx, cropped, asset.clone(), updated_item) {
        Ok(()) => {
            canvas.clear_crop_pending(updated_item_id);
            let indexed_asset = (semantic_search_enabled
                && crate::library::residency::asset_mime_type_supports_raster_derivatives(
                    &asset.mime_type,
                ))
            .then(|| (asset.id, asset.original_path.clone()));
            Ok(CroppedImageIntegration {
                updated_record,
                previous_record: previous_snapshot.0,
                indexed_asset,
            })
        }
        Err(error) => {
            canvas.clear_crop_pending(updated_item_id);
            Err(error)
        }
    }
}

fn cleanup_failed_crop(stored_path: &std::path::Path) {
    let _ = std::fs::remove_file(stored_path);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use eframe::egui;

    use crate::canvas::{CanvasCropRequest, CanvasState, PixelCropRect};
    use crate::data::{Asset, CanvasImageLod, Placement, PlacementState};
    use crate::jobs::{
        ImagePrepPreviewTexture, ImagePrepWorker, ImportOrigin, PreparedPlacementImage,
    };
    use crate::library::storage::Library;

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn enqueue_pending_crop_shows_cached_preview_before_worker_result() {
        let root = temp_library_root("crop-flow-pending-preview");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("source", &vec![255; 8 * 8 * 4], 8, 8)
            .unwrap();
        let ctx = egui::Context::default();
        let mut canvas = CanvasState::default();
        let mut placement = test_record(42, 42, 8, 8);
        placement.require_loaded_asset_mut().original_path = source_path.clone();
        let preview_rgba = (0..16)
            .flat_map(|index| [index, 0, 0, 255])
            .collect::<Vec<_>>();
        canvas.add_prepared_placement_image(
            &ctx,
            PreparedPlacementImage {
                target_board_id: 1,
                queue_id: None,
                placement: placement.clone(),
                preview_rgba,
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: ImagePrepPreviewTexture::Cpu,
                loaded_lod: CanvasImageLod::Working,
                existing_load_diagnostics: None,
                origin: ImportOrigin::Existing,
                trace: None,
            },
        );
        canvas.set_pending_crop_for_test(CanvasCropRequest {
            placement_id: 42,
            source_asset: placement.require_loaded_asset().clone(),
            crop_rect: PixelCropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            crop_offset_local: egui::vec2(2.0, 2.0),
        });
        let worker = ImagePrepWorker::new(library.clone(), ctx.clone(), None);

        let enqueued = super::enqueue_pending_requests(
            &ctx,
            &mut canvas,
            Some(&library),
            Some(&worker),
            4096,
            2000,
        )
        .unwrap();

        assert!(enqueued);
        assert_eq!(
            canvas.preview_for_placement_id(42).unwrap().texture_size,
            egui::vec2(2.0, 2.0)
        );
        let placement = canvas.placement_for_id(42).unwrap();
        assert_eq!(
            (
                placement.require_loaded_asset().width,
                placement.require_loaded_asset().height
            ),
            (4, 4)
        );
        assert_eq!(
            (
                placement.state.geometry.center.x,
                placement.state.geometry.center.y
            ),
            (2.0, 2.0)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn test_record(placement_id: i64, asset_id: i64, width: u32, height: u32) -> Placement {
        Placement::loaded(
            PlacementState {
                id: placement_id,
                asset_id: Some(asset_id),
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
                original_path: PathBuf::from(format!("image-{asset_id}.png")),
                filename: format!("image-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height,
            },
        )
    }

    fn temp_library_root(label: &str) -> PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("snappyref-{label}-{counter}"))
    }
}
