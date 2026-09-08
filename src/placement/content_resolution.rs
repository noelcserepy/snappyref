use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui;

use crate::data::Placement;
use crate::image_preparation::{ExistingLoadDiagnostics, ImagePrepTrace, ImportOrigin};

// Content Resolution correlates async worker results back to the Placement
// that reserved a canvas position. The queue id is the runtime handle; the
// Placement id is the durable object updated when loading succeeds or fails.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContentResolutionReservation {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
    pub(crate) placement_id: i64,
    pub(crate) reserved_world: egui::Pos2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentResolutionRequestConfig {
    pub(crate) max_texture_side: usize,
    pub(crate) canvas_image_max_side: u32,
    pub(crate) auto_image_scaling: bool,
    pub(crate) placement_target_area: f32,
}

#[derive(Default)]
pub(crate) struct PlacementCreation {
    pub(crate) batches: super::creation::PlacementCreationBatchTracker,
    next_queue_id: u64,
    pending: HashMap<u64, PendingContentResolution>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentResolutionCompletion {
    pub(crate) target_board_id: Option<i64>,
    pub(crate) queue_id: Option<u64>,
    pub(crate) placement_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContentResolutionTicket {
    pub(crate) target_board_id: i64,
    pub(crate) queue_id: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedPlacement {
    pub(crate) origin: ImportOrigin,
    pub(crate) placement: Placement,
    pub(crate) diagnostics: Option<ExistingLoadDiagnostics>,
    pub(crate) trace: Option<ImagePrepTrace>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedContentResolutionOutcome {
    pub(crate) resolution_ticket: Option<ContentResolutionTicket>,
    pub(crate) status: Option<String>,
    pub(crate) indexed_asset: Option<(i64, PathBuf)>,
    pub(crate) resolved: Option<ResolvedPlacement>,
}

#[derive(Clone, Copy, Debug)]
struct PendingContentResolution {
    target_board_id: i64,
    placement_id: i64,
    source: PendingContentResolutionSource,
    stale: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingContentResolutionSource {
    Import,
    HistoryReplay,
}

impl PlacementCreation {
    pub(crate) fn new() -> Self {
        Self {
            batches: Default::default(),
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
        self.reserve(
            target_board_id,
            placement_id,
            reserved_world,
            PendingContentResolutionSource::Import,
        )
    }

    fn reserve(
        &mut self,
        target_board_id: i64,
        placement_id: i64,
        reserved_world: egui::Pos2,
        source: PendingContentResolutionSource,
    ) -> ContentResolutionReservation {
        let queue_id = self.next_queue_id();
        self.pending.insert(
            queue_id,
            PendingContentResolution {
                target_board_id,
                placement_id,
                source,
                stale: false,
            },
        );
        ContentResolutionReservation {
            target_board_id,
            queue_id,
            placement_id,
            reserved_world,
        }
    }

    pub(crate) fn reserve_history_placement(
        &mut self,
        target_board_id: i64,
        placement_id: i64,
    ) -> ContentResolutionReservation {
        self.reserve(
            target_board_id,
            placement_id,
            egui::Pos2::ZERO,
            PendingContentResolutionSource::HistoryReplay,
        )
    }

    pub(crate) fn invalidate_placement(&mut self, placement_id: i64) {
        for pending in self.pending.values_mut() {
            if pending.placement_id != placement_id {
                continue;
            }
            pending.stale = true;
        }
    }

    pub(crate) fn placement_is_stale(&self, queue_id: u64, placement_id: i64) -> Option<bool> {
        match self.pending.get(&queue_id) {
            Some(PendingContentResolution {
                placement_id: pending_placement_id,
                stale,
                ..
            }) if *pending_placement_id == placement_id => Some(*stale),
            _ => None,
        }
    }

    pub(super) fn reservation_finished(&self, queue_id: u64) -> bool {
        queue_id < self.next_queue_id && !self.pending.contains_key(&queue_id)
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
        }
    }

    /// Removes the pending reservation and returns the batch completion plus
    /// the outcome to surface. History replays load silently: their status,
    /// index request, and diagnostics are dropped. Accepted content still completes.
    pub(crate) fn finish_loaded_import(
        &mut self,
        mut outcome: LoadedContentResolutionOutcome,
    ) -> (ContentResolutionCompletion, LoadedContentResolutionOutcome) {
        let ticket = outcome.resolution_ticket;
        let history_replay = ticket.is_some_and(|ticket| {
            self.pending.get(&ticket.queue_id).is_some_and(|pending| {
                matches!(
                    pending.source,
                    PendingContentResolutionSource::HistoryReplay
                )
            })
        });
        let completion = self.resolve_completion(
            ticket.map(|ticket| ticket.queue_id),
            ticket.map(|ticket| ticket.target_board_id),
            outcome.resolved.as_ref().map(|log| log.placement.state.id),
        );
        if history_replay {
            outcome.status = None;
            outcome.indexed_asset = None;
            if let Some(resolved) = &mut outcome.resolved {
                resolved.diagnostics = None;
                resolved.trace = None;
            }
        }
        (completion, outcome)
    }

    #[cfg(test)]
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

pub(crate) fn import_request(
    source: crate::image_preparation::ImportSource,
    reservation: ContentResolutionReservation,
    config: ContentResolutionRequestConfig,
    trace: Option<crate::image_preparation::ImagePrepTrace>,
) -> crate::image_preparation::ImagePrepRequest {
    crate::image_preparation::ImagePrepRequest::Import(crate::image_preparation::ImportRequest {
        target_board_id: reservation.target_board_id,
        queue_id: reservation.queue_id,
        placement_id: reservation.placement_id,
        source,
        reserved_world: reservation.reserved_world,
        max_texture_side: config.max_texture_side,
        canvas_image_settings: crate::image_preparation::CanvasImageSettings {
            max_side: config.canvas_image_max_side,
        },
        auto_scale: config.auto_image_scaling,
        placement_target_area: config.placement_target_area,
        trace,
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn reservation_tracks_queue_and_placement_correlation() {
        let mut runtime = super::PlacementCreation::new();

        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));

        assert_eq!(reservation.target_board_id, 2);
        assert_eq!(reservation.queue_id, 1);
        assert_eq!(reservation.placement_id, 41);
        assert_eq!(runtime.pending_len(), 1);
        let completion = runtime.resolve_completion(Some(1), None, None);
        assert_eq!(completion.target_board_id, Some(2));
        assert_eq!(completion.queue_id, Some(1));
        assert_eq!(completion.placement_id, Some(41));
        assert_eq!(runtime.pending_len(), 0);
    }

    #[test]
    fn cancel_removes_pending_resolution() {
        let mut runtime = super::PlacementCreation::new();
        let reservation = runtime.reserve_placement(1, 7, eframe::egui::Pos2::ZERO);

        runtime.cancel(reservation);

        let completion = runtime.resolve_completion(Some(reservation.queue_id), None, None);
        assert_eq!(completion.placement_id, None);
        assert_eq!(completion.target_board_id, None);
    }

    #[test]
    fn explicit_completion_values_win_over_pending_fallbacks() {
        let mut runtime = super::PlacementCreation::new();
        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));

        let completion = runtime.resolve_completion(Some(reservation.queue_id), Some(3), Some(42));

        assert_eq!(completion.target_board_id, Some(3));
        assert_eq!(completion.placement_id, Some(42));
        assert_eq!(runtime.pending_len(), 0);
    }

    #[test]
    fn import_path_request_carries_resolution_correlation() {
        let mut runtime = super::PlacementCreation::new();
        let reservation = runtime.reserve_placement(2, 41, eframe::egui::pos2(10.0, 20.0));

        let request = super::import_request(
            crate::image_preparation::ImportSource::Path(std::path::PathBuf::from("image.png")),
            reservation,
            super::ContentResolutionRequestConfig {
                max_texture_side: 512,
                canvas_image_max_side: 1024,
                auto_image_scaling: true,
                placement_target_area: 14400.0,
            },
            None,
        );

        let crate::image_preparation::ImagePrepRequest::Import(request) = request else {
            panic!("expected import request");
        };
        assert_eq!(request.target_board_id, 2);
        assert_eq!(request.queue_id, 1);
        assert_eq!(request.placement_id, 41);
        assert_eq!(request.reserved_world, eframe::egui::pos2(10.0, 20.0));
        assert_eq!(request.max_texture_side, 512);
        assert_eq!(request.canvas_image_settings.max_side, 1024);
        assert!(request.auto_scale);
        assert_eq!(request.placement_target_area, 14400.0);
    }
}
