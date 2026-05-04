const MAX_CROP_UNDO_ENTRIES: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct CropUndoEntry {
    pub(crate) previous_record: crate::data::BoardItemWithAsset,
    pub(crate) previous_rgba: Vec<u8>,
    pub(crate) previous_preview_width: u32,
    pub(crate) previous_preview_height: u32,
    pub(crate) removed_asset_id: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct CroppedImageIntegration {
    pub(crate) updated_record: crate::data::BoardItemWithAsset,
    pub(crate) indexed_asset: Option<(i64, std::path::PathBuf)>,
}

#[derive(Default)]
pub(crate) struct CropFlow {
    undo_stack: Vec<CropUndoEntry>,
}

impl CropFlow {
    pub(crate) fn clear_undo_stack(&mut self) {
        self.undo_stack.clear();
    }

    #[cfg(test)]
    pub(crate) fn undo_stack_is_empty(&self) -> bool {
        self.undo_stack.is_empty()
    }

    pub(crate) fn enqueue_pending_requests(
        &self,
        ctx: &eframe::egui::Context,
        canvas: &mut crate::canvas::CanvasState,
        import_worker: Option<&crate::jobs::ImportWorker>,
        max_texture_side: usize,
        canvas_image_max_side: u32,
    ) -> Result<bool, String> {
        let Some(import_worker) = import_worker else {
            return Ok(false);
        };

        let mut enqueued = false;
        while let Some(request) = canvas.take_pending_crop() {
            let item_id = request.item_id;
            canvas.mark_crop_pending(item_id);
            if let Err(error) = canvas.apply_pending_crop_preview(ctx, &request) {
                canvas.clear_crop_pending(item_id);
                return Err(error);
            }
            if let Err(error) = import_worker.enqueue(crate::jobs::ImportRequest::Crop {
                request,
                max_texture_side,
                canvas_image_settings: crate::jobs::CanvasImageSettings {
                    max_side: canvas_image_max_side,
                },
            }) {
                canvas.clear_crop_pending(item_id);
                return Err(error);
            }
            enqueued = true;
        }

        Ok(enqueued)
    }

    pub(crate) fn integrate_cropped_result(
        &mut self,
        ctx: &eframe::egui::Context,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::storage::Library>,
        cropped: crate::jobs::CroppedCanvasItem,
        _canvas_image_max_side: u32,
        semantic_search_enabled: bool,
    ) -> Result<CroppedImageIntegration, String> {
        let item_id = cropped.item_id;
        let stored_path = cropped.stored_path.clone();
        let previous_snapshot = match canvas.snapshot_item_image(item_id) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                cleanup_failed_crop(&stored_path);
                canvas.clear_crop_pending(item_id);
                return Err(error);
            }
        };

        let Some(library) = library else {
            cleanup_failed_crop(&stored_path);
            canvas.clear_crop_pending(item_id);
            return Err("Storage unavailable".to_owned());
        };

        let mut updated_item = match canvas.projected_cropped_item(&cropped) {
            Ok(item) => item,
            Err(error) => {
                cleanup_failed_crop(&stored_path);
                canvas.clear_crop_pending(item_id);
                return Err(error);
            }
        };

        let asset = match library.create_asset_and_relink_board_item(
            updated_item.id,
            &stored_path,
            image::ImageFormat::Png.to_mime_type(),
            cropped.width,
            cropped.height,
            cropped.file_size,
            eframe::egui::pos2(updated_item.x, updated_item.y),
        ) {
            Ok(asset) => asset,
            Err(error) => {
                cleanup_failed_crop(&stored_path);
                canvas.clear_crop_pending(item_id);
                return Err(error);
            }
        };

        updated_item.asset_id = asset.id;

        let updated_item_id = updated_item.id;
        let updated_record = crate::data::BoardItemWithAsset {
            item: updated_item.clone(),
            asset: asset.clone(),
        };

        match canvas.apply_cropped_item(ctx, cropped, asset.clone(), updated_item) {
            Ok(()) => {
                self.push_undo_entry(CropUndoEntry {
                    previous_record: previous_snapshot.0,
                    previous_rgba: previous_snapshot.1,
                    previous_preview_width: previous_snapshot.2,
                    previous_preview_height: previous_snapshot.3,
                    removed_asset_id: asset.id,
                });
                canvas.clear_crop_pending(updated_item_id);
                let indexed_asset = (semantic_search_enabled
                    && crate::residency::asset_mime_type_supports_raster_derivatives(
                        &asset.mime_type,
                    ))
                .then(|| (asset.id, asset.original_path.clone()));
                Ok(CroppedImageIntegration {
                    updated_record,
                    indexed_asset,
                })
            }
            Err(error) => {
                canvas.clear_crop_pending(updated_item_id);
                Err(error)
            }
        }
    }

    pub(crate) fn undo_last_crop(
        &mut self,
        ctx: &eframe::egui::Context,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::storage::Library>,
        active_board_id: i64,
    ) -> Result<bool, String> {
        let Some(library) = library else {
            return Err("Storage unavailable".to_owned());
        };
        let Some(entry) = self.undo_stack.pop() else {
            return Ok(false);
        };

        let removed_path = match library.restore_board_item_and_maybe_remove_asset(
            &entry.previous_record.item,
            entry.removed_asset_id,
        ) {
            Ok(path) => path,
            Err(error) => {
                self.undo_stack.push(entry);
                return Err(error);
            }
        };

        if let Some(path) = removed_path {
            let _ = std::fs::remove_file(&path);
            let _ = library.remove_asset_canvas_caches(entry.removed_asset_id);
            canvas.evict_cached_asset(entry.removed_asset_id);
        }

        canvas.add_loaded_item(
            ctx,
            crate::jobs::LoadedCanvasItem {
                target_board_id: active_board_id,
                queue_id: None,
                preview_width: entry.previous_preview_width,
                preview_height: entry.previous_preview_height,
                record: entry.previous_record,
                preview_rgba: entry.previous_rgba,
                animated_preview: None,
                preview_texture: crate::jobs::ImportedPreviewTexture::Cpu,
                loaded_tier: crate::data::CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: crate::jobs::ImportOrigin::Existing,
                trace: None,
            },
        );

        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn push_test_undo_entry(&mut self, entry: CropUndoEntry) {
        self.push_undo_entry(entry);
    }

    fn push_undo_entry(&mut self, entry: CropUndoEntry) {
        self.undo_stack.push(entry);
        if self.undo_stack.len() > MAX_CROP_UNDO_ENTRIES {
            self.undo_stack.remove(0);
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

    use super::CropFlow;
    use crate::canvas::{CanvasCropRequest, CanvasState, PixelCropRect};
    use crate::data::{BoardItem, BoardItemWithAsset, CanvasImageCacheTier, ImageAsset};
    use crate::jobs::{ImportOrigin, ImportWorker, ImportedPreviewTexture, LoadedCanvasItem};
    use crate::storage::Library;

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
        let mut record = test_record(42, 42, 8, 8);
        record.asset.original_path = source_path.clone();
        let preview_rgba = (0..16)
            .flat_map(|index| [index, 0, 0, 255])
            .collect::<Vec<_>>();
        canvas.add_loaded_item(
            &ctx,
            LoadedCanvasItem {
                target_board_id: 1,
                queue_id: None,
                record: record.clone(),
                preview_rgba,
                preview_width: 4,
                preview_height: 4,
                animated_preview: None,
                preview_texture: ImportedPreviewTexture::Cpu,
                loaded_tier: CanvasImageCacheTier::Working,
                existing_load_diagnostics: None,
                origin: ImportOrigin::Existing,
                trace: None,
            },
        );
        canvas.set_pending_crop_for_test(CanvasCropRequest {
            item_id: 42,
            source_asset: record.asset,
            crop_rect: PixelCropRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            crop_offset_local: egui::vec2(2.0, 2.0),
        });
        let worker = ImportWorker::new(library, ctx.clone(), None);

        let enqueued = CropFlow::default()
            .enqueue_pending_requests(&ctx, &mut canvas, Some(&worker), 4096, 2000)
            .unwrap();

        assert!(enqueued);
        assert_eq!(
            canvas.preview_for_item_id(42).unwrap().texture_size,
            egui::vec2(2.0, 2.0)
        );
        let record = canvas.record_for_item_id(42).unwrap();
        assert_eq!((record.asset.width, record.asset.height), (4, 4));
        assert_eq!((record.item.x, record.item.y), (2.0, 2.0));

        let _ = std::fs::remove_dir_all(root);
    }

    fn test_record(item_id: i64, asset_id: i64, width: u32, height: u32) -> BoardItemWithAsset {
        BoardItemWithAsset {
            item: BoardItem {
                id: item_id,
                asset_id,
                x: 0.0,
                y: 0.0,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
            asset: ImageAsset {
                id: asset_id,
                original_path: PathBuf::from(format!("image-{asset_id}.png")),
                filename: format!("image-{asset_id}.png"),
                mime_type: "image/png".to_owned(),
                width,
                height,
                file_size: 0,
                imported_at: 0,
            },
        }
    }

    fn temp_library_root(label: &str) -> PathBuf {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("snappyref-{label}-{counter}"))
    }
}
