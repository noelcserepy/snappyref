use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

use crate::canvas::CanvasState;
use crate::data::BoardItemWithAsset;
use crate::jobs::{ExistingLoadDiagnostics, ImportOrigin, ImportTrace};
use crate::storage::Library;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContentResolutionReservation {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
    pub(crate) placement_id: Option<i64>,
    pub(crate) reserved_world: Option<egui::Pos2>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentResolutionRequestConfig {
    pub(crate) max_texture_side: usize,
    pub(crate) canvas_image_max_side: u32,
    pub(crate) auto_image_scaling: bool,
    pub(crate) import_target_canvas_area: f32,
    pub(crate) import_gap: f32,
}

#[derive(Default)]
pub(crate) struct ContentResolutionRuntime {
    next_queue_id: u64,
    pending: HashMap<u64, PendingContentResolution>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentResolutionCompletion {
    pub(crate) target_board_id: Option<i64>,
    pub(crate) queue_id: Option<u64>,
    pub(crate) placement_id: Option<i64>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) reserved_world: Option<egui::Pos2>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImportPlaceholderRemoval {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImportLog {
    pub(crate) origin: ImportOrigin,
    pub(crate) record: BoardItemWithAsset,
    pub(crate) diagnostics: Option<ExistingLoadDiagnostics>,
    pub(crate) trace: Option<ImportTrace>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedImportOutcome {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) status: Option<String>,
    pub(crate) indexed_asset: Option<(i64, PathBuf)>,
    pub(crate) log: Option<LoadedImportLog>,
}

#[derive(Clone, Debug)]
pub(crate) struct FailedImportOutcome {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) placement_id: Option<i64>,
    pub(crate) target_board_id: Option<i64>,
    pub(crate) status: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FinishedLoadedImport {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) batch_target_board_id: Option<i64>,
    pub(crate) batch_queue_id: Option<u64>,
    pub(crate) loaded_placement_id: Option<i64>,
    pub(crate) status: Option<String>,
    pub(crate) indexed_asset: Option<(i64, PathBuf)>,
    pub(crate) log: Option<LoadedImportLog>,
}

#[derive(Clone, Debug)]
pub(crate) struct FinishedFailedImport {
    pub(crate) placeholder_removal: Option<ImportPlaceholderRemoval>,
    pub(crate) batch_target_board_id: Option<i64>,
    pub(crate) batch_queue_id: Option<u64>,
    pub(crate) status: String,
}

#[derive(Clone, Copy, Debug)]
struct PendingContentResolution {
    target_board_id: i64,
    placement_id: i64,
    reserved_world: egui::Pos2,
}

impl ContentResolutionRuntime {
    pub(crate) fn new() -> Self {
        Self {
            next_queue_id: 1,
            pending: HashMap::new(),
        }
    }

    pub(crate) fn reserve_placement(
        &mut self,
        target_board_id: i64,
        placement_id: i64,
        reserved_world: egui::Pos2,
    ) -> ContentResolutionReservation {
        let queue_id = self.next_queue_id();
        self.pending.insert(
            queue_id,
            PendingContentResolution {
                target_board_id,
                placement_id,
                reserved_world,
            },
        );
        ContentResolutionReservation {
            target_board_id,
            queue_id,
            placement_id: Some(placement_id),
            reserved_world: Some(reserved_world),
        }
    }

    pub(crate) fn reserve_untracked(
        &mut self,
        target_board_id: i64,
        reserved_world: egui::Pos2,
    ) -> ContentResolutionReservation {
        ContentResolutionReservation {
            target_board_id,
            queue_id: self.next_queue_id(),
            placement_id: None,
            reserved_world: Some(reserved_world),
        }
    }

    pub(crate) fn cancel(&mut self, reservation: ContentResolutionReservation) {
        self.pending.remove(&reservation.queue_id);
    }

    pub(crate) fn resolve_completion(
        &mut self,
        queue_id: Option<u64>,
        target_board_id: Option<i64>,
        placement_id: Option<i64>,
    ) -> ContentResolutionCompletion {
        let finished = queue_id.and_then(|queue_id| self.pending.remove(&queue_id));
        ContentResolutionCompletion {
            target_board_id: target_board_id
                .or_else(|| finished.as_ref().map(|pending| pending.target_board_id)),
            queue_id,
            placement_id: placement_id
                .or_else(|| finished.as_ref().map(|pending| pending.placement_id)),
            reserved_world: finished.map(|pending| pending.reserved_world),
        }
    }

    pub(crate) fn finish_loaded_import(
        &mut self,
        outcome: LoadedImportOutcome,
    ) -> FinishedLoadedImport {
        let target_board_id = outcome
            .placeholder_removal
            .as_ref()
            .map(|placeholder| placeholder.target_board_id);
        let queue_id = outcome
            .placeholder_removal
            .as_ref()
            .map(|placeholder| placeholder.queue_id);
        let completion = self.resolve_completion(
            queue_id,
            target_board_id,
            outcome.log.as_ref().map(|log| log.record.item.id),
        );
        FinishedLoadedImport {
            placeholder_removal: outcome.placeholder_removal,
            batch_target_board_id: completion.target_board_id,
            batch_queue_id: completion.queue_id,
            loaded_placement_id: completion.placement_id,
            status: outcome.status,
            indexed_asset: outcome.indexed_asset,
            log: outcome.log,
        }
    }

    pub(crate) fn finish_failed_import(
        &mut self,
        library: Option<&Library>,
        canvas: &mut CanvasState,
        active_board_id: i64,
        outcome: FailedImportOutcome,
    ) -> FinishedFailedImport {
        let target_board_id = outcome
            .placeholder_removal
            .as_ref()
            .map(|placeholder| placeholder.target_board_id);
        let queue_id = outcome
            .placeholder_removal
            .as_ref()
            .map(|placeholder| placeholder.queue_id);
        let completion = self.resolve_completion(
            queue_id,
            target_board_id.or(outcome.target_board_id),
            outcome.placement_id,
        );
        if let (Some(library), Some(placement_id)) = (library, completion.placement_id) {
            let failed_board_id = completion.target_board_id.unwrap_or(active_board_id);
            if let Ok(Some(record)) =
                library.mark_placement_failed(failed_board_id, placement_id, outcome.status.clone())
                && failed_board_id == active_board_id
                && canvas.record_for_item_id(placement_id).is_some()
            {
                canvas.add_unloaded_item(record);
                canvas.select_items(&[placement_id]);
            }
        }
        FinishedFailedImport {
            placeholder_removal: outcome.placeholder_removal,
            batch_target_board_id: completion.target_board_id,
            batch_queue_id: completion.queue_id,
            status: outcome.status,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn next_queue_id(&mut self) -> u64 {
        if self.next_queue_id == 0 {
            self.next_queue_id = 1;
        }
        let queue_id = self.next_queue_id;
        self.next_queue_id = self.next_queue_id.saturating_add(1);
        queue_id
    }
}

pub(crate) fn import_path_request(
    source_path: PathBuf,
    reservation: ContentResolutionReservation,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    config: ContentResolutionRequestConfig,
    trace: Option<crate::jobs::ImportTrace>,
) -> crate::jobs::ImportRequest {
    crate::jobs::ImportRequest::ImportPath {
        target_board_id: reservation.target_board_id,
        queue_id: reservation.queue_id,
        placement_id: reservation.placement_id,
        source_path,
        desired_world,
        reserved_world: reservation.reserved_world,
        viewport_world,
        max_texture_side: config.max_texture_side,
        canvas_image_settings: canvas_image_settings(config),
        auto_scale: config.auto_image_scaling,
        target_canvas_area: config.import_target_canvas_area,
        import_gap: config.import_gap,
        trace,
    }
}

pub(crate) fn import_url_request(
    url: String,
    reservation: ContentResolutionReservation,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    config: ContentResolutionRequestConfig,
    trace: Option<crate::jobs::ImportTrace>,
) -> crate::jobs::ImportRequest {
    crate::jobs::ImportRequest::ImportUrl {
        target_board_id: reservation.target_board_id,
        queue_id: reservation.queue_id,
        placement_id: reservation.placement_id,
        url,
        desired_world,
        reserved_world: reservation.reserved_world,
        viewport_world,
        max_texture_side: config.max_texture_side,
        canvas_image_settings: canvas_image_settings(config),
        auto_scale: config.auto_image_scaling,
        target_canvas_area: config.import_target_canvas_area,
        import_gap: config.import_gap,
        trace,
    }
}

pub(crate) fn import_clipboard_request(
    rgba: Vec<u8>,
    width: u32,
    height: u32,
    reservation: ContentResolutionReservation,
    desired_world: egui::Pos2,
    viewport_world: egui::Rect,
    config: ContentResolutionRequestConfig,
    trace: Option<crate::jobs::ImportTrace>,
) -> crate::jobs::ImportRequest {
    crate::jobs::ImportRequest::ImportClipboard {
        target_board_id: reservation.target_board_id,
        queue_id: reservation.queue_id,
        placement_id: reservation.placement_id,
        rgba,
        width,
        height,
        desired_world,
        reserved_world: reservation.reserved_world,
        viewport_world,
        max_texture_side: config.max_texture_side,
        canvas_image_settings: canvas_image_settings(config),
        auto_scale: config.auto_image_scaling,
        target_canvas_area: config.import_target_canvas_area,
        import_gap: config.import_gap,
        trace,
    }
}

fn canvas_image_settings(
    config: ContentResolutionRequestConfig,
) -> crate::jobs::CanvasImageSettings {
    crate::jobs::CanvasImageSettings {
        max_side: config.canvas_image_max_side,
    }
}

#[cfg(test)]
mod tests {
    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn reservation_tracks_queue_and_placement_correlation() {
        let mut runtime = super::ContentResolutionRuntime::new();

        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));

        assert_eq!(reservation.target_board_id, 2);
        assert_eq!(reservation.queue_id, 1);
        assert_eq!(reservation.placement_id, Some(41));
        assert_eq!(runtime.pending_len(), 1);
        let completion = runtime.resolve_completion(Some(1), None, None);
        assert_eq!(completion.target_board_id, Some(2));
        assert_eq!(completion.queue_id, Some(1));
        assert_eq!(completion.placement_id, Some(41));
        assert_eq!(
            completion.reserved_world,
            Some(eframe::egui::pos2(10.0, 20.0))
        );
        assert_eq!(runtime.pending_len(), 0);
    }

    #[test]
    fn cancel_removes_pending_resolution() {
        let mut runtime = super::ContentResolutionRuntime::new();
        let reservation = runtime.reserve_placement(1, 7, eframe::egui::Pos2::ZERO);

        runtime.cancel(reservation);

        let completion = runtime.resolve_completion(Some(reservation.queue_id), None, None);
        assert_eq!(completion.placement_id, None);
        assert_eq!(completion.target_board_id, None);
        assert_eq!(completion.reserved_world, None);
    }

    #[test]
    fn explicit_completion_values_win_over_pending_fallbacks() {
        let mut runtime = super::ContentResolutionRuntime::new();
        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));

        let completion = runtime.resolve_completion(Some(reservation.queue_id), Some(3), Some(42));

        assert_eq!(completion.target_board_id, Some(3));
        assert_eq!(completion.placement_id, Some(42));
        assert_eq!(
            completion.reserved_world,
            Some(eframe::egui::pos2(10.0, 20.0))
        );
        assert_eq!(runtime.pending_len(), 0);
    }

    #[test]
    fn import_path_request_carries_resolution_correlation() {
        let mut runtime = super::ContentResolutionRuntime::new();
        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));
        let viewport_world = eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(100.0, 80.0),
        );

        let request = super::import_path_request(
            std::path::PathBuf::from("image.png"),
            reservation,
            eframe::egui::pos2(12.0, 24.0),
            viewport_world,
            super::ContentResolutionRequestConfig {
                max_texture_side: 512,
                canvas_image_max_side: 1024,
                auto_image_scaling: true,
                import_target_canvas_area: 14400.0,
                import_gap: 18.0,
            },
            None,
        );

        match request {
            crate::jobs::ImportRequest::ImportPath {
                target_board_id,
                queue_id,
                placement_id,
                reserved_world,
                max_texture_side,
                canvas_image_settings,
                auto_scale,
                target_canvas_area,
                import_gap,
                ..
            } => {
                assert_eq!(target_board_id, 2);
                assert_eq!(queue_id, 1);
                assert_eq!(placement_id, Some(41));
                assert_eq!(reserved_world, Some(eframe::egui::pos2(10.0, 20.0)));
                assert_eq!(max_texture_side, 512);
                assert_eq!(canvas_image_settings.max_side, 1024);
                assert!(auto_scale);
                assert_eq!(target_canvas_area, 14400.0);
                assert_eq!(import_gap, 18.0);
            }
            _ => panic!("expected import path request"),
        }
    }

    #[test]
    fn failed_import_marks_correlated_loading_placement_failed() {
        let root = temp_root("snappyref-content-resolution-failure");
        let library = crate::storage::Library::initialize(root.clone()).unwrap();
        let loading = library
            .persist_committed_loading_placement(
                eframe::egui::pos2(10.0, 20.0),
                1.0,
                100.0,
                80.0,
                crate::data::PlacementContentIntent::Unknown {
                    description: "test".to_owned(),
                },
                Some("Queued".to_owned()),
            )
            .unwrap();
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.add_unloaded_item(loading.clone());
        let mut runtime = super::ContentResolutionRuntime::new();
        let reservation = runtime.reserve_placement(1, loading.item.id, eframe::egui::Pos2::ZERO);

        let finished = runtime.finish_failed_import(
            Some(&library),
            &mut canvas,
            1,
            super::FailedImportOutcome {
                placeholder_removal: Some(super::ImportPlaceholderRemoval {
                    target_board_id: 1,
                    queue_id: reservation.queue_id,
                }),
                placement_id: None,
                target_board_id: None,
                status: "decode failed".to_owned(),
            },
        );

        assert_eq!(finished.batch_target_board_id, Some(1));
        assert_eq!(finished.batch_queue_id, Some(reservation.queue_id));
        let stored = library
            .load_board_items()
            .unwrap()
            .into_iter()
            .find(|record| record.item.id == loading.item.id)
            .unwrap();
        assert!(matches!(
            stored.content,
            crate::data::PlacementContent::Failed { .. }
        ));
        let canvas_record = canvas.record_for_item_id(loading.item.id).unwrap();
        assert!(matches!(
            canvas_record.content,
            crate::data::PlacementContent::Failed { .. }
        ));

        let _ = std::fs::remove_dir_all(root);
    }
}
