//! About / Updates update flow, the restart-into-update handoff, and the
//! scripted update smoke run.

use eframe::egui;
use std::time::Duration;
use std::time::Instant;

use crate::updater::UpdateState;

use super::semantic_search::semantic_search_settings_status;
use super::{AppConfig, SnappyRefApp};

/// Minimum wall time between About / Updates update-check clicks. Discovery is
/// opportunistic; the manual button is a convenience, not a way to hammer the
/// manifest endpoint.
pub(super) const MANUAL_UPDATE_CHECK_THROTTLE: Duration = Duration::from_secs(60);

/// Locate the macOS `snappyref-updater` helper next to the running executable.
/// The helper ships inside the same `MacOS/` directory as the main `snappyref`
/// binary, so the stable lookup is by sibling path from `current_exe`.
#[cfg(not(target_os = "windows"))]
pub(super) fn snappyref_updater_path() -> Result<std::path::PathBuf, String> {
    let current_exe = std::env::current_exe().map_err(|error| format!("current_exe: {error}"))?;
    let parent = current_exe
        .parent()
        .ok_or_else(|| "current_exe has no parent directory".to_owned())?;
    Ok(parent.join("snappyref-updater"))
}

pub(super) fn restart_launch_args() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|argument| argument != "--update-smoke-install")
        .collect()
}

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

impl SnappyRefApp {
    /// Build the About / Updates view payload from the
    /// internal updater state machine.
    pub(super) fn about_updates_view(&self) -> crate::ui::sidebar::AboutUpdatesView {
        let check_now_enabled = match self.last_manual_update_check {
            None => true,
            Some(instant) => instant.elapsed() >= MANUAL_UPDATE_CHECK_THROTTLE,
        };
        let (update_button_label, update_button_enabled, update_button_primary, update_status) =
            self.about_update_action_view(check_now_enabled);
        crate::ui::sidebar::AboutUpdatesView {
            current_version: self.update_service.current_version().to_owned(),
            last_checked_unix_millis: self.update_service.last_check_completed_at(),
            update_button_label,
            update_button_enabled,
            update_button_primary,
            update_status,
            semantic_search: crate::ui::sidebar::SemanticSearchSettingsView {
                status: semantic_search_settings_status(self.search.local_model_store_status()),
            },
        }
    }

    pub(super) fn about_update_action_view(
        &self,
        check_now_enabled: bool,
    ) -> (String, bool, bool, Option<String>) {
        if self.update_service.check_in_flight() {
            return (
                "Checking for updates".to_owned(),
                false,
                false,
                Some("Checking for updates...".to_owned()),
            );
        }
        match self.update_service.state() {
            UpdateState::Hidden => {
                let status = self
                    .update_service
                    .latest_known()
                    .then(|| "You are on the latest version.".to_owned());
                (
                    "Check for updates".to_owned(),
                    check_now_enabled,
                    false,
                    status,
                )
            }
            UpdateState::Available { version, .. } => (
                "Update".to_owned(),
                true,
                true,
                Some(format!("Version {version} is available.")),
            ),
            UpdateState::Downloading { percent, .. } => (
                "Updating".to_owned(),
                false,
                false,
                Some(format!("Downloading {percent}%")),
            ),
            UpdateState::Ready { .. } if self.user_update_flow_active => (
                "Finishing update".to_owned(),
                false,
                false,
                Some("Restarting to finish update...".to_owned()),
            ),
            UpdateState::Ready { .. } => (
                "Restart".to_owned(),
                true,
                true,
                Some("Update ready. Restart to finish.".to_owned()),
            ),
            UpdateState::Failed => (
                "Check for updates".to_owned(),
                check_now_enabled,
                false,
                Some("Update check failed.".to_owned()),
            ),
        }
    }

    /// Mirror the update service's most recent check timestamp into the
    /// persisted Workspace Settings so it survives across launches.
    /// Returns true when the value changed and the caller should persist.
    pub(super) fn sync_last_update_check_timestamp(&mut self) -> bool {
        let current = self.update_service.last_check_completed_at();
        if current != self.settings.last_update_check_at {
            self.settings.last_update_check_at = current;
            true
        } else {
            false
        }
    }

    pub(super) fn handle_update_action_click(&mut self, ctx: &egui::Context) {
        match self.update_service.state().clone() {
            UpdateState::Available { .. } => {
                self.user_update_flow_active = true;
                self.update_service.start_install();
                ctx.request_repaint();
            }
            UpdateState::Ready { .. } => {
                self.user_update_flow_active = true;
                self.trigger_restart_into_update(ctx);
            }
            UpdateState::Downloading { .. } => {}
            UpdateState::Hidden | UpdateState::Failed => {
                self.request_manual_update_check(ctx);
            }
        }
    }

    /// Process a About / Updates update check click. Throttled to one
    /// invocation per [`MANUAL_UPDATE_CHECK_THROTTLE`].
    pub(super) fn request_manual_update_check(&mut self, ctx: &egui::Context) {
        if !self.update_service.check_in_flight() {
            let now = Instant::now();
            if let Some(last) = self.last_manual_update_check {
                if now.duration_since(last) < MANUAL_UPDATE_CHECK_THROTTLE {
                    return;
                }
            }
            self.last_manual_update_check = Some(now);
            self.update_service.request_check_now();
        }
        ctx.request_repaint();
    }

    pub(super) fn drive_user_update_flow(&mut self, ctx: &egui::Context) {
        if !self.user_update_flow_active {
            return;
        }
        match self.update_service.state().clone() {
            UpdateState::Ready { .. } => self.trigger_restart_into_update(ctx),
            UpdateState::Failed => self.user_update_flow_active = false,
            UpdateState::Downloading { .. } => {
                ctx.request_repaint_after(Duration::from_millis(250))
            }
            _ => {}
        }
    }

    pub(super) fn write_update_smoke_status(&self, status: &str) {
        let Some(path) = self
            .update_smoke
            .as_ref()
            .and_then(|smoke| smoke.status_path.as_ref())
        else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, status);
    }

    pub(super) fn fail_update_smoke(&mut self, ctx: &egui::Context, reason: impl Into<String>) {
        let reason = reason.into();
        self.status = format!("Update smoke failed: {reason}");
        self.write_update_smoke_status(&format!("failed:{reason}"));
        self.update_smoke = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub(super) fn drive_update_smoke(&mut self, ctx: &egui::Context, settled_idle: bool) {
        let Some(smoke) = self.update_smoke.as_ref() else {
            return;
        };
        if smoke.started_at.elapsed() > smoke.timeout {
            let phase = smoke.phase_label();
            self.fail_update_smoke(ctx, format!("timeout in {phase}"));
            return;
        }

        match smoke.phase {
            UpdateSmokePhase::WaitingForIdle => {
                if !settled_idle {
                    ctx.request_repaint_after(Duration::from_millis(250));
                    return;
                }
                self.write_update_smoke_status("checking");
                self.update_service.request_check_now();
                if let Some(smoke) = &mut self.update_smoke {
                    smoke.phase = UpdateSmokePhase::WaitingForAvailable;
                }
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            UpdateSmokePhase::WaitingForAvailable => match self.update_service.state().clone() {
                UpdateState::Available { version, .. } => {
                    self.write_update_smoke_status(&format!("available:{version}"));
                    self.update_service.start_install();
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::WaitingForReady;
                    }
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
                UpdateState::Ready { version } => {
                    self.write_update_smoke_status(&format!("triggered:{version}"));
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::TriggeredRestart;
                    }
                    self.trigger_restart_into_update(ctx);
                }
                UpdateState::Failed => {
                    self.fail_update_smoke(ctx, "update service failed");
                }
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            },
            UpdateSmokePhase::WaitingForReady => match self.update_service.state().clone() {
                UpdateState::Ready { version } => {
                    self.write_update_smoke_status(&format!("triggered:{version}"));
                    if let Some(smoke) = &mut self.update_smoke {
                        smoke.phase = UpdateSmokePhase::TriggeredRestart;
                    }
                    self.trigger_restart_into_update(ctx);
                }
                UpdateState::Failed => {
                    self.fail_update_smoke(ctx, "update service failed");
                }
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            },
            UpdateSmokePhase::TriggeredRestart => {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    /// Drive the swap-and-relaunch flow when the user clicks Restart in
    /// Settings:
    ///
    /// 1. close any open update affordance.
    /// 2. save the User Session — the new app's first launch reads it back.
    /// 3. drop worker child processes (embedding helper, import worker)
    ///    so the swap is not racing live filesystem writers.
    /// 4. apply the staged update using the platform's swap path:
    ///    Windows launches a temporary self-replace runner; macOS writes
    ///    a pending-update descriptor and launches `snappyref-updater`.
    pub(super) fn trigger_restart_into_update(&mut self, _ctx: &egui::Context) {
        self.user_update_flow_active = false;

        // 2. Save the User Session so the new app can restore it.
        self.save_active_board_before_transition();

        // 3. Stop worker child processes. ImagePrepWorker's queue + thread
        // shutdown runs in its Drop impl; the embedding helper's stdin
        // child process is killed by EmbeddingHelperClient::drop, which
        // fires when the SearchController is dropped or rebuilt.
        self.import_worker = None;
        {
            self.search.shutdown_helper_processes();
        }

        // 4. Build + write the descriptor.
        let launch_args = restart_launch_args();
        let Some(descriptor) = self.update_service.pending_restart_descriptor(launch_args) else {
            self.status = "No staged update is ready to restart into".to_owned();
            return;
        };
        #[cfg(target_os = "windows")]
        {
            let descriptor_path =
                crate::updater::descriptor::default_path(&descriptor.install_root);
            if let Err(error) = crate::updater::windows_self_replace::apply_and_relaunch(
                &descriptor,
                &descriptor_path,
            ) {
                if self.update_smoke.is_some() {
                    self.fail_update_smoke(_ctx, format!("apply failed: {error}"));
                    return;
                }
                self.status = format!("Failed to apply Windows update: {error}");
                return;
            }
            std::process::exit(0);
        }
        #[cfg(not(target_os = "windows"))]
        {
            let descriptor_path =
                crate::updater::descriptor::default_path(&descriptor.install_root);
            if let Err(error) = crate::updater::descriptor::write(&descriptor_path, &descriptor) {
                self.status = format!("Failed to write update descriptor: {error}");
                return;
            }

            // 5. Spawn the helper detached. argv[1] is the descriptor path.
            let helper_path = match snappyref_updater_path() {
                Ok(path) => path,
                Err(error) => {
                    self.status = format!("Failed to locate updater helper: {error}");
                    return;
                }
            };
            if let Err(error) = std::process::Command::new(&helper_path)
                .arg(&descriptor_path)
                .spawn()
            {
                self.status = format!("Failed to launch updater helper: {error}");
                return;
            }

            // 6. Tell egui to close — the helper polls our PID and starts the
            // swap as soon as we exit.
            _ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
