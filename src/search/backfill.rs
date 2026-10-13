use std::time::{Duration, Instant};

use super::controller::SearchController;
use crate::search::semantic::IndexResult;

const EMBEDDING_BACKFILL_IDLE_QUIET_MS: u64 = 750;
/// Storage gives each Asset a few attempts. This many failed steps with no
/// step embedding an Asset in between means the helper or storage is broken,
/// so the scheduler stops before it spends every Asset's attempts.
const MAX_CONSECUTIVE_BACKFILL_FAILURES: u8 = 3;

// Embedding backfill is idle-only background work. Demand embeddings for an
// import/search action run through the service immediately; this scheduler
// waits for app work and search work to drain before filling missing Library
// embeddings so interactive frames stay storage-light.
#[derive(Default)]
pub(super) struct EmbeddingBackfillScheduler {
    pub(super) quiet_since: Option<Instant>,
    pub(super) in_flight: bool,
    /// No step is worth asking for until `retry_at`, or until other work such
    /// as an import or a Board switch clears it.
    pub(super) exhausted: bool,
    /// When the earliest stored retry comes due.
    retry_at: Option<Instant>,
    /// Failed steps since the last step that embedded an Asset. Imports do not
    /// reset it: a re-import of an indexed Asset succeeds without the helper.
    consecutive_failures: u8,
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
        if self
            .embedding_backfill
            .retry_at
            .is_some_and(|retry_at| now >= retry_at)
        {
            self.embedding_backfill.retry_at = None;
            self.embedding_backfill.exhausted = false;
        }
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

    /// Applies an index result to the scheduler. Every finished step releases
    /// `in_flight`. A failed step does not stop the backfill: storage took the
    /// Asset out of fresh work, so the next step moves on, and stored retries
    /// follow once fresh work is done.
    pub(super) fn note_index_result_for_backfill(&mut self, result: &IndexResult, now: Instant) {
        let backfill = &mut self.embedding_backfill;
        match result {
            IndexResult::BackfillIndexed => backfill.consecutive_failures = 0,
            IndexResult::BackfillIdle { retry_in } => {
                backfill.exhausted = true;
                backfill.retry_at = retry_in.map(|retry_in| now + retry_in);
            }
            IndexResult::BackfillFailed { .. } => {
                backfill.consecutive_failures = backfill.consecutive_failures.saturating_add(1);
                if backfill.consecutive_failures >= MAX_CONSECUTIVE_BACKFILL_FAILURES {
                    backfill.exhausted = true;
                    backfill.retry_at = None;
                }
            }
            _ => return,
        }
        backfill.in_flight = false;
    }

    /// How long until the scheduler wants another frame. `None` while a step
    /// is in flight or there is nothing left to wait for.
    pub(crate) fn embedding_backfill_repaint_after(&self, now: Instant) -> Option<Duration> {
        let backfill = &self.embedding_backfill;
        if self.service.is_none() || backfill.in_flight {
            None
        } else if backfill.exhausted {
            backfill
                .retry_at
                .map(|retry_at| retry_at.saturating_duration_since(now))
        } else {
            Some(Duration::from_millis(EMBEDDING_BACKFILL_IDLE_QUIET_MS))
        }
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
