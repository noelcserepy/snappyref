use std::time::{Duration, Instant};

use super::controller::SearchController;

const EMBEDDING_BACKFILL_IDLE_QUIET_MS: u64 = 750;

// Embedding backfill is idle-only background work. Demand embeddings for an
// import/search action run through the service immediately; this scheduler
// waits for app work and search work to drain before filling missing Library
// embeddings so interactive frames stay storage-light.
#[derive(Default)]
pub(super) struct EmbeddingBackfillScheduler {
    pub(super) quiet_since: Option<Instant>,
    pub(super) in_flight: bool,
    pub(super) exhausted: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EmbeddingBackfillIdleState {
    pub(crate) app_idle: bool,
}

impl SearchController {
    pub(crate) fn can_start_embedding_backfill(
        &mut self,
        now: Instant,
        idle: EmbeddingBackfillIdleState,
    ) -> bool {
        if self.embedding_backfill.exhausted
            || self.embedding_backfill.in_flight
            || self.service.is_none()
            || !idle.app_idle
            || self.search_work_in_flight(now)
        {
            self.embedding_backfill.quiet_since = None;
            return false;
        }

        let quiet_since = *self.embedding_backfill.quiet_since.get_or_insert(now);
        now.duration_since(quiet_since) >= Duration::from_millis(EMBEDDING_BACKFILL_IDLE_QUIET_MS)
    }

    pub(crate) fn drive_embedding_backfill(
        &mut self,
        now: Instant,
        idle: EmbeddingBackfillIdleState,
    ) -> Result<bool, String> {
        if !self.can_start_embedding_backfill(now, idle) {
            return Ok(false);
        }
        let Some(service) = &self.service else {
            return Ok(false);
        };
        service.enqueue_idle_backfill_step()?;
        self.embedding_backfill.in_flight = true;
        self.embedding_backfill.quiet_since = None;
        Ok(true)
    }

    pub(crate) fn embedding_backfill_may_need_repaint(&self) -> bool {
        self.service.is_some()
            && !self.embedding_backfill.exhausted
            && !self.embedding_backfill.in_flight
    }

    pub(crate) fn reset_embedding_backfill(&mut self) {
        self.embedding_backfill = EmbeddingBackfillScheduler::default();
    }

    pub(super) fn note_demand_embedding_drained_at(&mut self, now: Instant) {
        self.embedding_backfill.quiet_since = now
            .checked_sub(Duration::from_millis(EMBEDDING_BACKFILL_IDLE_QUIET_MS))
            .or(Some(now));
    }
}
