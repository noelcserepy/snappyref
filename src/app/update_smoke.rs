use std::time::{Duration, Instant};

use super::AppConfig;

#[derive(Clone, Copy, Debug)]
pub(super) enum UpdateSmokePhase {
    WaitingForIdle,
    WaitingForAvailable,
    WaitingForReady,
    TriggeredRestart,
}

#[derive(Clone, Debug)]
pub(super) struct UpdateSmokeState {
    pub(super) phase: UpdateSmokePhase,
    pub(super) started_at: Instant,
    pub(super) timeout: Duration,
    pub(super) status_path: Option<std::path::PathBuf>,
}

impl UpdateSmokeState {
    pub(super) fn from_config(config: &AppConfig) -> Option<Self> {
        if !config.update_smoke_install {
            return None;
        }
        let timeout = std::env::var("SNAPPYREF_UPDATE_SMOKE_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(300));
        Some(Self {
            phase: UpdateSmokePhase::WaitingForIdle,
            started_at: Instant::now(),
            timeout,
            status_path: config.update_smoke_status_path.clone(),
        })
    }

    pub(super) fn phase_label(&self) -> &'static str {
        match self.phase {
            UpdateSmokePhase::WaitingForIdle => "waiting_for_idle",
            UpdateSmokePhase::WaitingForAvailable => "waiting_for_available",
            UpdateSmokePhase::WaitingForReady => "waiting_for_ready",
            UpdateSmokePhase::TriggeredRestart => "triggered_restart",
        }
    }
}
