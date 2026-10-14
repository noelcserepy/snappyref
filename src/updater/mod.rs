//! Auto-update discovery, download, staging, and restart-apply orchestration.
//!
//! The runtime [`UpdateService`] runs on the egui update loop:
//!
//! 1. After the app reaches Settled Idle, it fetches `latest.json` from
//!    the configured manifest URL on a background thread so startup is
//!    never blocked.
//! 2. The response bytes are signature-verified against the public key
//!    embedded at compile time, then parsed.
//! 3. If the manifest version is greater than the running version, the
//!    update state enters Available; otherwise it stays Hidden.
//! 4. Failures back off exponentially up to 6 hours and never raise a
//!    popup. Hidden becomes Failed; known available, downloading, and ready
//!    updates retain their state.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod descriptor;
pub(crate) mod discovery;
pub(crate) mod download;
pub(crate) mod install_paths;
pub(crate) mod manifest;
pub(crate) mod platform;
pub(crate) mod stager;
pub(crate) mod state;
pub(crate) mod version;
#[cfg(any(target_os = "windows", test))]
pub(crate) mod windows_self_replace;

pub(crate) use state::UpdateState;

use discovery::{NextAction, ScheduleInput, decide};
use manifest::Manifest;
#[cfg(test)]
use manifest::PlatformEntry;
use platform::TargetPlatform;
use state::{CheckOutcome, InstallProgress};

/// URL of the signed manifest published by CI to this repo's release. The
/// "latest" alias resolves to the most recent published, non-prerelease release.
pub(crate) const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/noelcserepy/snappyref/releases/latest/download/latest.json";

/// Network/parse timeout for the fetch worker. Keep small so a hung TCP
/// connection cannot keep a background thread alive past the next 6h tick.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Runtime state for the update service. Owned by the app and ticked from
/// the main update loop.
pub(crate) struct UpdateService {
    state: UpdateState,
    started_at: Instant,
    settled_idle: bool,
    last_check: Option<Duration>,
    consecutive_failures: u32,
    in_flight: Option<Receiver<CheckOutcome>>,
    next_check_after: Option<Duration>,
    config: UpdateConfig,
    /// Wall-clock unix millis at which the most recent discovery check
    /// completed (success or failure). About / Updates renders this; `None`
    /// before the first check returns.
    last_check_completed_at: Option<u64>,
    /// Set to true when [`request_check_now`] fires; consumed by the next
    /// tick to force a discovery cycle outside the steady-state schedule.
    manual_check_requested: bool,
    /// Install-worker progress receiver. `None` while no install is in
    /// flight. A finished install drops the channel via `Failed` or `Staged`.
    install_rx: Option<Receiver<InstallProgress>>,
    /// Test override: short-circuit the network and feed a canned outcome
    /// instead of spawning a thread. See `spawn_fetch`.
    #[cfg(test)]
    fetch_override: Option<Box<dyn FnMut() -> CheckOutcome + Send>>,
    /// Test override: feed install events directly. See `spawn_install`.
    #[cfg(test)]
    install_override: Option<Box<dyn FnMut(InstallContext, Sender<InstallProgress>) + Send>>,
}

#[derive(Clone)]
pub(crate) struct UpdateConfig {
    pub manifest_url: String,
    pub current_version: String,
    /// Install root override, applied to the in-app updates/staging
    /// resolution. `None` resolves via `install_paths::resolve_install_root`.
    pub install_root: Option<PathBuf>,
    pub diagnostic_path: Option<PathBuf>,
}

#[cfg(test)]
impl UpdateConfig {
    /// Fixed config so tests never read the `SNAPPYREF_UPDATE_*` environment
    /// or append to a real diagnostic log.
    pub(crate) fn for_tests() -> Self {
        Self {
            manifest_url: "https://example.invalid/manifest.json".to_owned(),
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            install_root: None,
            diagnostic_path: None,
        }
    }
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            manifest_url: std::env::var("SNAPPYREF_UPDATE_MANIFEST_URL")
                .unwrap_or_else(|_| DEFAULT_MANIFEST_URL.to_owned()),
            current_version: std::env::var("SNAPPYREF_UPDATE_CURRENT_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned()),
            install_root: None,
            diagnostic_path: std::env::var_os("SNAPPYREF_UPDATE_DIAGNOSTIC_PATH")
                .map(PathBuf::from),
        }
    }
}

/// Payload handed to the install worker. Self-contained so the worker
/// closure has everything it needs without borrowing from the service.
#[derive(Clone, Debug)]
pub(crate) struct InstallContext {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub install_root: PathBuf,
    pub platform: TargetPlatform,
}

impl UpdateService {
    pub(crate) fn new(config: UpdateConfig) -> Self {
        if let Some(path) = &config.diagnostic_path {
            write_diagnostic(
                path,
                &format!(
                    "new current_version={} manifest_url={}",
                    config.current_version, config.manifest_url
                ),
            );
        }
        let service = Self {
            state: UpdateState::Hidden,
            started_at: Instant::now(),
            settled_idle: false,
            last_check: None,
            consecutive_failures: 0,
            in_flight: None,
            next_check_after: None,
            config,
            last_check_completed_at: None,
            manual_check_requested: false,
            install_rx: None,
            #[cfg(test)]
            fetch_override: None,
            #[cfg(test)]
            install_override: None,
        };
        // Best-effort startup hygiene: discard partial downloads from a
        // prior session so a half-written file cannot be served as a
        // verified update. Tests inject install_root via config.
        service.cleanup_partial_downloads();
        service
    }

    /// Build the pending-update descriptor for the currently-staged
    /// version, if the update UI is in `Ready` state. Returns `None` if there
    /// is nothing to restart into, the host platform has no release
    /// shape, or the install root cannot be resolved.
    ///
    /// `launch_args` should be the argv tail (everything after argv[0])
    /// replayed by the macOS helper or Windows self-replacement runner.
    pub(crate) fn pending_restart_descriptor(
        &self,
        launch_args: Vec<String>,
    ) -> Option<descriptor::PendingUpdateDescriptor> {
        let version = match self.state() {
            UpdateState::Ready { version } => version.clone(),
            _ => return None,
        };
        let install_root = self.resolved_install_root()?;
        let platform = TargetPlatform::current()?;
        let staging_dir = install_paths::staging_dir(&install_root, &version);
        let staged_bundle = staged_swap_unit(&staging_dir, platform);
        let current_bundle = current_swap_unit(&install_root, platform);
        Some(descriptor::PendingUpdateDescriptor {
            parent_pid: std::process::id(),
            expected_new_version: version,
            install_root,
            current_bundle,
            staged_bundle,
            staging_dir,
            launch_args,
        })
    }

    /// Drop any in-flight zip file inside `<install_root>/updates/` that
    /// is not a finished staging directory. Called once on construction.
    fn cleanup_partial_downloads(&self) {
        let Some(install_root) = self.resolved_install_root() else {
            return;
        };
        let updates_dir = install_paths::updates_dir(&install_root);
        let Ok(entries) = std::fs::read_dir(&updates_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // Only remove regular files. Staging directories are kept;
            // their verification is checked separately at startup.
            if file_type.is_file() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn resolved_install_root(&self) -> Option<PathBuf> {
        if let Some(root) = &self.config.install_root {
            return Some(root.clone());
        }
        install_paths::resolve_install_root().ok()
    }

    /// Start the real download + verify + stage flow for the current
    /// Available update. Called when the user clicks Update in Settings.
    pub(crate) fn start_install(&mut self) {
        // Only allowed while we have a verified manifest and the update UI
        // is showing Available.
        let manifest = match &self.state {
            UpdateState::Available { release } => release.clone(),
            _ => return,
        };
        let Some(platform) = TargetPlatform::current() else {
            // No update package is available for this OS.
            return;
        };
        let entry = manifest.entry_for(platform);
        let Some(install_root) = self.resolved_install_root() else {
            self.state = UpdateState::Failed {
                reason: "Cannot resolve the app install location".to_owned(),
                version: Some(manifest.version),
            };
            return;
        };
        let context = InstallContext {
            version: manifest.version.clone(),
            url: entry.url.clone(),
            sha256: entry.sha256.clone(),
            size: entry.size,
            install_root,
            platform,
        };
        self.state.begin_install(context.version.clone());
        let (tx, rx) = channel();
        self.install_rx = Some(rx);
        self.spawn_install(context, tx);
    }

    /// Tests never download: without an override the install fails
    /// immediately instead of touching the network.
    fn spawn_install(&mut self, context: InstallContext, tx: Sender<InstallProgress>) {
        #[cfg(test)]
        if let Some(installer) = self.install_override.as_mut() {
            (installer)(context, tx);
            return;
        }
        if cfg!(test) {
            let _ = tx.send(InstallProgress::Failed {
                reason: "no test installer".to_owned(),
            });
        } else {
            std::thread::spawn(move || run_install(context, tx));
        }
    }

    /// Check whether a verified staged bundle for `manifest.version` is
    /// already on disk and, if so, mark the update UI Ready directly. Called
    /// from the discovery hook so it runs once per verified manifest.
    ///
    /// Also sweeps stale `staging-{other-version}/` directories left
    /// behind by prior sessions whose manifest version no longer matches.
    /// An in-flight download's staging dir is preserved so its install
    /// worker is not interrupted.
    fn detect_existing_staging(&mut self, manifest: &Manifest) {
        let Some(install_root) = self.resolved_install_root() else {
            return;
        };
        let Some(platform) = TargetPlatform::current() else {
            return;
        };
        self.sweep_stale_staging(&install_root, &manifest.version);
        let staging = install_paths::staging_dir(&install_root, &manifest.version);
        if !staging.is_dir() {
            return;
        }
        // Validate the staged layout matches what restart-apply will swap into
        // place. We only accept a structurally-valid bundle as Ready;
        // otherwise leave the update UI alone.
        if stager::validate_layout(&staging.join(stager::EXPECTED_TOP_LEVEL), platform).is_err() {
            // Stale or corrupt staging directory — clean it up so the
            // next download has a clean target.
            let _ = std::fs::remove_dir_all(&staging);
            return;
        }
        // Re-verify the source-zip sha recorded at stage time matches the
        // manifest's per-platform sha. A mismatch means the manifest was
        // re-published with a different binary for this version, or the
        // staging predates the sidecar; either way we must not promote.
        let expected_sha = manifest.entry_for(platform).sha256.trim().to_lowercase();
        let staged_sha = stager::read_source_sha(&staging);
        if staged_sha.as_deref() != Some(expected_sha.as_str()) {
            let _ = std::fs::remove_dir_all(&staging);
            return;
        }
        self.state.mark_ready(manifest.version.clone());
    }

    /// Remove `staging-{version}/` directories inside `<install_root>/updates/`
    /// whose version is neither the manifest's current version nor an
    /// in-flight download's version. Prevents indefinite accumulation of
    /// staged bundles from prior versions.
    fn sweep_stale_staging(&self, install_root: &std::path::Path, manifest_version: &str) {
        let updates_dir = install_paths::updates_dir(install_root);
        let Ok(entries) = std::fs::read_dir(&updates_dir) else {
            return;
        };
        let keep_for_manifest = format!("staging-{manifest_version}");
        let keep_for_download = match &self.state {
            UpdateState::Downloading { version, .. } if version != manifest_version => {
                Some(format!("staging-{version}"))
            }
            _ => None,
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with("staging-") {
                continue;
            }
            if name_str == keep_for_manifest {
                continue;
            }
            if keep_for_download
                .as_deref()
                .is_some_and(|keep| keep == name_str)
            {
                continue;
            }
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }

    pub(crate) fn state(&self) -> &UpdateState {
        &self.state
    }

    pub(crate) fn current_version(&self) -> &str {
        &self.config.current_version
    }

    pub(crate) fn check_in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(crate) fn latest_known(&self) -> bool {
        matches!(self.state, UpdateState::Hidden)
            && self.last_check_completed_at.is_some()
            && self.consecutive_failures == 0
            && self.in_flight.is_none()
    }

    /// Called once per egui frame. `app_settled_idle` should be true once
    /// the app's queues have drained for the first time since startup —
    /// the same signal the existing perf-scenario logic uses.
    pub(crate) fn tick(&mut self, app_settled_idle: bool) {
        if app_settled_idle && !self.settled_idle {
            self.settled_idle = true;
            self.write_diagnostic("settled_idle");
        }

        if let Some(rx) = &self.in_flight {
            match rx.try_recv() {
                Ok(result) => {
                    self.in_flight = None;
                    self.last_check = Some(self.started_at.elapsed());
                    self.last_check_completed_at = current_unix_millis();
                    match &result {
                        CheckOutcome::Failed => self.consecutive_failures += 1,
                        CheckOutcome::NewerAvailable { .. } | CheckOutcome::UpToDate => {
                            self.consecutive_failures = 0;
                        }
                    }
                    self.write_diagnostic(&format!(
                        "result outcome={:?} state_before={:?}",
                        result, self.state
                    ));
                    let discovered_version_during_download = match (&self.state, &result) {
                        (
                            UpdateState::Downloading { .. },
                            CheckOutcome::NewerAvailable { release },
                        ) => Some(release.version.clone()),
                        _ => None,
                    };
                    self.state.apply(result);
                    self.write_diagnostic(&format!("state_after={:?}", self.state));
                    if let UpdateState::Available { release } = &self.state {
                        let release = release.clone();
                        self.detect_existing_staging(&release);
                    } else if let Some(version) = discovered_version_during_download
                        && let Some(root) = self.resolved_install_root()
                    {
                        self.sweep_stale_staging(&root, &version);
                    }
                }
                Err(TryRecvError::Empty) => {
                    self.drain_install_progress();
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.in_flight = None;
                    self.last_check = Some(self.started_at.elapsed());
                    self.last_check_completed_at = current_unix_millis();
                    self.consecutive_failures += 1;
                    self.state.apply(CheckOutcome::Failed);
                }
            }
        }

        self.drain_install_progress();

        // The manual About / Updates action jumps the
        // queue regardless of the schedule. Throttling is the caller's
        // responsibility — the UI gates the button to one click per
        // minute so a wedged network does not get pinged repeatedly.
        if self.manual_check_requested {
            self.manual_check_requested = false;
            self.start_check();
            return;
        }

        let input = ScheduleInput {
            settled_idle: self.settled_idle,
            now: self.started_at.elapsed(),
            last_check: self.last_check,
            consecutive_failures: self.consecutive_failures,
        };
        match decide(input) {
            NextAction::CheckNow => self.start_check(),
            NextAction::WaitFor(d) => self.next_check_after = Some(d),
            NextAction::WaitForSettledIdle => self.next_check_after = None,
        }
    }

    /// Schedule an immediate discovery cycle the next time [`tick`] runs,
    /// bypassing the steady-state poll interval. Used by the Settings
    /// Tab "Check for updates" button.
    pub(crate) fn request_check_now(&mut self) {
        self.manual_check_requested = true;
        self.write_diagnostic("manual_check_requested");
    }

    /// Unix-epoch milliseconds the most recent check completed. `None`
    /// before any check has resolved.
    pub(crate) fn last_check_completed_at(&self) -> Option<u64> {
        self.last_check_completed_at
    }

    /// Restore the persisted last-check timestamp on app launch. Called
    /// by [`crate::app::SnappyRefApp`] right after the User Session
    /// loads so the About / Updates can render the timestamp before the
    /// first discovery cycle of this session completes.
    pub(crate) fn seed_last_check_completed_at(&mut self, unix_millis: Option<u64>) {
        if self.last_check_completed_at.is_none() {
            self.last_check_completed_at = unix_millis;
        }
    }

    /// Puts the service in `state` as if a check had just completed at
    /// `checked_unix_millis`, so the schedule waits a full interval.
    #[cfg(test)]
    pub(crate) fn set_checked_state_for_test(
        &mut self,
        state: UpdateState,
        checked_unix_millis: u64,
    ) {
        self.state = state;
        self.last_check = Some(self.started_at.elapsed());
        self.last_check_completed_at = Some(checked_unix_millis);
    }

    /// Suggested wait until updater work needs another tick, used by
    /// callers to request a future repaint. Poll while waiting for the
    /// first settled-idle sample and while a worker is in flight so
    /// discovery cannot stall in a quiescent UI.
    pub(crate) fn time_until_next_check(&self) -> Option<Duration> {
        if !self.settled_idle {
            return Some(Duration::from_millis(250));
        }
        if self.in_flight.is_some() || self.install_rx.is_some() {
            return Some(Duration::from_millis(250));
        }
        self.next_check_after
    }

    fn start_check(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.write_diagnostic("start_check");
        self.spawn_fetch(tx);
        self.in_flight = Some(rx);
        self.next_check_after = None;
    }

    /// Tests never reach the network: without an override the check fails
    /// immediately instead of spawning the fetch thread.
    fn spawn_fetch(&mut self, tx: Sender<CheckOutcome>) {
        #[cfg(test)]
        if let Some(fetcher) = self.fetch_override.as_mut() {
            let outcome = (fetcher)();
            let _ = tx.send(outcome);
            return;
        }
        if cfg!(test) {
            let _ = tx.send(CheckOutcome::Failed);
        } else {
            let config = self.config.clone();
            std::thread::spawn(move || {
                let outcome = run_fetch(&config);
                let _ = tx.send(outcome);
            });
        }
    }

    fn write_diagnostic(&self, message: &str) {
        if let Some(path) = &self.config.diagnostic_path {
            write_diagnostic(path, message);
        }
    }

    /// Drain any install-worker progress events that have arrived since
    /// the last tick. The state machine deduplicates by version so a
    /// stale message from a previous install is ignored automatically.
    fn drain_install_progress(&mut self) {
        let Some(rx) = &self.install_rx else { return };
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(progress) => {
                    let was_terminal = matches!(
                        &progress,
                        InstallProgress::Staged { .. } | InstallProgress::Failed { .. }
                    );
                    self.state.apply_progress(progress);
                    if was_terminal {
                        finished = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Worker exited without sending Staged/Failed. Treat
                    // as a failure so the update UI is not stuck mid-progress.
                    if matches!(self.state, UpdateState::Downloading { .. }) {
                        self.state.apply_progress(InstallProgress::Failed {
                            reason: "install worker exited".to_owned(),
                        });
                    }
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.install_rx = None;
        }
    }
}

/// Resolve the swap unit inside a staging tree:
/// `<staging>/SnappyRef/SnappyRef.app` on macOS,
/// `<staging>/SnappyRef/snappyref.exe` on Windows.
pub(crate) fn staged_swap_unit(staging_dir: &std::path::Path, platform: TargetPlatform) -> PathBuf {
    staging_dir
        .join(stager::EXPECTED_TOP_LEVEL)
        .join(platform.swap_unit_name())
}

/// Resolve the swap unit inside the live install root:
/// `<install_root>/SnappyRef.app` on macOS,
/// `<install_root>/snappyref.exe` on Windows.
pub(crate) fn current_swap_unit(
    install_root: &std::path::Path,
    platform: TargetPlatform,
) -> PathBuf {
    install_root.join(platform.swap_unit_name())
}

/// Production install pipeline: download the zip, verify sha, extract
/// into staging, validate layout. Each phase reports progress through
/// `tx`. Drop semantics on the channel act as a failure signal too.
fn run_install(context: InstallContext, tx: Sender<InstallProgress>) {
    let zip_path = install_paths::download_file_path(&context.install_root, &context.version);
    let version_for_progress = context.version.clone();
    let progress_tx = tx.clone();
    let mut last_pct: u8 = 0;
    let last_emit = std::cell::Cell::new(std::time::Instant::now());
    let mut progress = move |done: u64, total: u64| {
        let pct = if total == 0 {
            0
        } else {
            ((done as f64 / total as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        };
        // Forward each percent change, or after 500 ms, to limit repaints.
        let now = std::time::Instant::now();
        if pct != last_pct
            || now.saturating_duration_since(last_emit.get()) >= Duration::from_millis(500)
        {
            last_pct = pct;
            last_emit.set(now);
            let _ = progress_tx.send(InstallProgress::Progress {
                version: version_for_progress.clone(),
                percent: pct,
            });
        }
    };

    match download::download_from_url(
        &context.url,
        &zip_path,
        context.size,
        &context.sha256,
        &mut progress,
    ) {
        Ok(_) => {}
        Err(error) => {
            log_failure("download", &error);
            let _ = tx.send(InstallProgress::Failed {
                reason: error.to_string(),
            });
            return;
        }
    }

    let platform = context.platform;

    let staging_root = install_paths::staging_dir(&context.install_root, &context.version);
    match stager::extract_and_validate(&zip_path, &staging_root, platform) {
        Ok(_) => {
            // Verified zip is no longer needed; remove it so the only
            // on-disk artifact for this version is the staging tree.
            let _ = std::fs::remove_file(&zip_path);
            // Record the source-zip sha so a future session can re-verify
            // the staging directory still matches the manifest before
            // promoting it to Ready.
            stager::record_source_sha(&staging_root, &context.sha256);
            let _ = tx.send(InstallProgress::Staged {
                version: context.version,
            });
        }
        Err(error) => {
            log_failure("stage", &error);
            let _ = std::fs::remove_file(&zip_path);
            let _ = tx.send(InstallProgress::Failed {
                reason: error.to_string(),
            });
        }
    }
}

fn run_fetch(config: &UpdateConfig) -> CheckOutcome {
    if let Some(path) = &config.diagnostic_path {
        write_diagnostic(path, "fetch_begin");
    }
    let bytes = match fetch_manifest(&config.manifest_url) {
        Ok(bytes) => bytes,
        Err(error) => {
            if let Some(path) = &config.diagnostic_path {
                write_diagnostic(path, &format!("fetch_failed {error}"));
            }
            log_failure("fetch", &error);
            return CheckOutcome::Failed;
        }
    };
    let manifest = match manifest::parse_and_verify(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            if let Some(path) = &config.diagnostic_path {
                write_diagnostic(path, &format!("verify_failed {error}"));
            }
            log_failure("verify", &error);
            return CheckOutcome::Failed;
        }
    };
    let outcome = classify(&manifest, &config.current_version);
    if let Some(path) = &config.diagnostic_path {
        write_diagnostic(
            path,
            &format!(
                "fetch_ok manifest_version={} current_version={} outcome={outcome:?}",
                manifest.version, config.current_version
            ),
        );
    }
    outcome
}

fn fetch_manifest(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|error| format!("client: {error}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| format!("send: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("status: {}", response.status()));
    }
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|error| format!("body: {error}"))
}

fn current_unix_millis() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn log_failure(stage: &str, error: &dyn std::fmt::Display) {
    // Update discovery failures are quiet: logging only, no popup.
    eprintln!("update discovery {stage} failed: {error}");
}

fn write_diagnostic(path: &Path, message: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = writeln!(file, "{message}");
}

/// Compare a verified manifest's version against `current_version` and
/// translate into the outcome the state machine consumes.
fn classify(manifest: &Manifest, current_version: &str) -> CheckOutcome {
    if version::is_newer_than(&manifest.version, current_version) {
        CheckOutcome::NewerAvailable {
            release: manifest.clone(),
        }
    } else {
        CheckOutcome::UpToDate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn drive_until_quiescent(service: &mut UpdateService) {
        // Pump tick() up to a small bound; each tick can complete one
        // in-flight check via the synchronous test fetcher.
        for _ in 0..32 {
            service.tick(true);
            if service.in_flight.is_none() && service.time_until_next_check().is_some() {
                return;
            }
        }
    }

    #[test]
    fn service_waits_through_long_startup_then_checks_once_at_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::NewerAvailable {
                release: manifest_with("2.0.0"),
            }
        }));

        service.started_at = Instant::now() - Duration::from_secs(60);
        for _ in 0..16 {
            service.tick(false);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(*service.state(), UpdateState::Hidden);

        service.tick(true);
        drive_until_quiescent(&mut service);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            service.state(),
            &UpdateState::Available {
                release: manifest_with("2.0.0")
            }
        );
    }

    #[test]
    fn discovery_failure_reports_failed_and_retries_within_a_minute() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| CheckOutcome::Failed));
        drive_until_quiescent(&mut service);
        assert_eq!(
            service.state(),
            &UpdateState::Failed {
                reason: "Release discovery failed".into(),
                version: None
            }
        );
        assert!(service.time_until_next_check().unwrap() <= Duration::from_secs(60));
    }

    pub(super) fn manifest_with(version: &str) -> Manifest {
        Manifest {
            version: version.to_owned(),
            macos: PlatformEntry {
                url: "https://example/mac.zip".to_owned(),
                sha256: "0".repeat(64),
                size: 1,
            },
            windows: PlatformEntry {
                url: "https://example/win.zip".to_owned(),
                sha256: "0".repeat(64),
                size: 1,
            },
        }
    }

    #[test]
    fn retained_available_release_keeps_its_exact_package_after_older_discovery() {
        for later_version in ["0.5.0", "1.0.0", "1.5.0", "2.0.0"] {
            let root = TempRoot::new("retained-release");
            let mut service = UpdateService::new(UpdateConfig {
                current_version: "1.0.0".to_owned(),
                install_root: Some(root.to_path_buf()),
                ..UpdateConfig::for_tests()
            });
            let mut offered = manifest_with("2.0.0");
            offered.macos.url = "https://example/offered-mac.zip".into();
            offered.windows.url = "https://example/offered-win.zip".into();
            offered.macos.sha256 = "a".repeat(64);
            offered.windows.sha256 = "b".repeat(64);
            let later = manifest_with(later_version);
            let mut responses = std::collections::VecDeque::from([
                classify(&offered, "1.0.0"),
                classify(&later, "1.0.0"),
            ]);
            service.fetch_override = Some(Box::new(move || responses.pop_front().unwrap()));
            drive_until_quiescent(&mut service);
            service.request_check_now();
            drive_until_quiescent(&mut service);
            assert_eq!(
                service.state(),
                &UpdateState::Available {
                    release: offered.clone()
                }
            );
            service.install_override = Some(Box::new(|context, _| {
                assert_eq!(context.version, "2.0.0");
                assert_eq!(context.size, 1);
                #[cfg(target_os = "macos")]
                {
                    assert_eq!(context.platform, TargetPlatform::Macos);
                    assert_eq!(context.url, "https://example/offered-mac.zip");
                    assert_eq!(
                        context.sha256,
                        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    );
                }
                #[cfg(target_os = "windows")]
                {
                    assert_eq!(context.platform, TargetPlatform::Windows);
                    assert_eq!(context.url, "https://example/offered-win.zip");
                    assert_eq!(
                        context.sha256,
                        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                    );
                }
            }));
            service.start_install();
            assert_eq!(
                service.state(),
                &UpdateState::Downloading {
                    version: "2.0.0".into(),
                    percent: 0
                }
            );
        }
    }

    #[test]
    fn preparation_failure_keeps_reason_and_requires_a_fresh_check() {
        let root = TempRoot::new("failed-release-recheck");
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "1.0.0".into(),
            install_root: Some(root.to_path_buf()),
            ..UpdateConfig::for_tests()
        });
        service.fetch_override = Some(Box::new(|| classify(&manifest_with("2.0.0"), "1.0.0")));
        drive_until_quiescent(&mut service);
        service.install_override = Some(Box::new(|_, tx| {
            tx.send(InstallProgress::Failed {
                reason: "Package checksum mismatch".into(),
            })
            .unwrap();
        }));
        service.start_install();
        service.drain_install_progress();
        assert_eq!(
            service.state(),
            &UpdateState::Failed {
                reason: "Package checksum mismatch".into(),
                version: Some("2.0.0".into()),
            }
        );
        service.start_install();
        assert_eq!(
            service.state(),
            &UpdateState::Failed {
                reason: "Package checksum mismatch".into(),
                version: Some("2.0.0".into())
            }
        );
        service.request_check_now();
        drive_until_quiescent(&mut service);
        assert_eq!(
            service.state(),
            &UpdateState::Available {
                release: manifest_with("2.0.0")
            }
        );
    }

    #[test]
    fn drain_install_progress_promotes_to_ready_on_staged() {
        let root = TempRoot::new("install-progress");
        let mut service = UpdateService::new(UpdateConfig {
            install_root: Some(root.to_path_buf()),
            ..UpdateConfig::for_tests()
        });
        service.state.apply(CheckOutcome::NewerAvailable {
            release: manifest_with("1.0.0"),
        });
        // Test installer immediately reports progress + Staged.
        service.install_override = Some(Box::new(|ctx, tx| {
            let _ = tx.send(InstallProgress::Progress {
                version: ctx.version.clone(),
                percent: 50,
            });
            let _ = tx.send(InstallProgress::Staged {
                version: ctx.version,
            });
        }));
        service.start_install();
        service.tick(true);
        assert_eq!(
            service.state(),
            &UpdateState::Ready {
                version: "1.0.0".into()
            }
        );
    }

    #[test]
    fn cleanup_partial_downloads_removes_files_in_updates_dir() {
        let root = TempRoot::new("updater-cleanup");
        // A partial zip left behind by a previous launch.
        let updates_dir = install_paths::updates_dir(&root);
        std::fs::create_dir_all(&updates_dir).unwrap();
        let partial = updates_dir.join("download-1.0.0.zip");
        std::fs::write(&partial, b"partial").unwrap();
        // And a staging dir that must NOT be touched.
        let staging = install_paths::staging_dir(&root, "0.9.0");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("marker"), b"keep").unwrap();

        let _service = UpdateService::new(UpdateConfig {
            install_root: Some(root.to_path_buf()),
            ..UpdateConfig::for_tests()
        });

        assert!(!partial.exists(), "partial download must be removed");
        assert_eq!(std::fs::read(staging.join("marker")).unwrap(), b"keep");
    }

    /// Create a structurally-valid staged bundle on disk, with the source
    /// sha sidecar when `sha` is given. Returns the staging directory.
    fn write_valid_staging(
        install_root: &std::path::Path,
        version: &str,
        sha: Option<&str>,
    ) -> PathBuf {
        let staging = install_paths::staging_dir(install_root, version);
        match TargetPlatform::current().expect("test runs on macOS or Windows") {
            TargetPlatform::Macos => {
                let macos = staging
                    .join(stager::EXPECTED_TOP_LEVEL)
                    .join("SnappyRef.app")
                    .join("Contents")
                    .join("MacOS");
                std::fs::create_dir_all(&macos).unwrap();
                std::fs::write(macos.join("snappyref"), b"binary").unwrap();
                std::fs::write(macos.join("snappyref-updater"), b"helper").unwrap();
            }
            TargetPlatform::Windows => {
                let bundle = staging.join(stager::EXPECTED_TOP_LEVEL);
                std::fs::create_dir_all(&bundle).unwrap();
                std::fs::write(bundle.join("snappyref.exe"), b"binary").unwrap();
            }
        }
        if let Some(sha) = sha {
            stager::record_source_sha(&staging, sha);
        }
        staging
    }

    /// Run one discovery that offers 1.0.0, whose package sha is
    /// `"0".repeat(64)`, to an app on 0.1.0.
    fn discover_release(root: &TempRoot) -> UpdateService {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(root.to_path_buf()),
            ..UpdateConfig::for_tests()
        });
        service.fetch_override = Some(Box::new(|| classify(&manifest_with("1.0.0"), "0.1.0")));
        drive_until_quiescent(&mut service);
        service
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detect_existing_staging_rejects_missing_app_or_helper() {
        for executable in ["snappyref", "snappyref-updater"] {
            let root = TempRoot::new("restart-missing-executable");
            let staging = write_valid_staging(&root, "9.9.9", Some(&"0".repeat(64)));
            std::fs::remove_file(staging.join(format!(
                "SnappyRef/SnappyRef.app/Contents/MacOS/{executable}"
            )))
            .unwrap();
            let mut service = UpdateService::new(UpdateConfig {
                install_root: Some(root.to_path_buf()),
                ..UpdateConfig::for_tests()
            });
            service.detect_existing_staging(&manifest_with("9.9.9"));
            assert_eq!(service.state(), &UpdateState::Hidden);
            assert!(!staging.exists());
        }
    }

    #[test]
    fn detect_existing_staging_promotes_to_ready_when_manifest_matches() {
        let root = TempRoot::new("updater-staged");
        write_valid_staging(&root, "1.0.0", Some(&"0".repeat(64)));

        let service = discover_release(&root);

        assert_eq!(
            service.state(),
            &UpdateState::Ready {
                version: "1.0.0".into()
            }
        );
    }

    #[test]
    fn detect_existing_staging_discards_a_bundle_without_the_manifest_sha() {
        // No sidecar means the sha was lost; a different sha means the
        // version was republished. Neither proves the bundle matches.
        let republished = "f".repeat(64);
        for sidecar in [None, Some(republished.as_str())] {
            let root = TempRoot::new("updater-staging-sha");
            let staging = write_valid_staging(&root, "1.0.0", sidecar);

            let service = discover_release(&root);

            assert_eq!(
                service.state(),
                &UpdateState::Available {
                    release: manifest_with("1.0.0")
                }
            );
            assert!(
                !staging.exists(),
                "sidecar {sidecar:?} staging must be removed"
            );
        }
    }

    #[test]
    fn detect_existing_staging_sweeps_stale_versions_after_manifest_fetch() {
        let root = TempRoot::new("updater-sweep");
        let stale = install_paths::staging_dir(&root, "0.9.0");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("debris"), b"old").unwrap();
        let current = write_valid_staging(&root, "1.0.0", Some(&"0".repeat(64)));

        discover_release(&root);

        assert!(!stale.exists(), "stale staging-0.9.0/ must be removed");
        let payload = if cfg!(target_os = "macos") {
            "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref"
        } else {
            "SnappyRef/snappyref.exe"
        };
        assert_eq!(std::fs::read(current.join(payload)).unwrap(), b"binary");
    }

    #[test]
    fn last_check_completed_at_records_a_timestamp_after_a_check_returns() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| CheckOutcome::UpToDate));
        assert_eq!(service.last_check_completed_at(), None);
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        drive_until_quiescent(&mut service);
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let completed = service.last_check_completed_at().unwrap();
        assert!(
            (before..=after).contains(&completed),
            "completion timestamp {completed} outside {before}..={after}"
        );
        assert_eq!(service.state(), &UpdateState::Hidden);
    }

    #[test]
    fn seed_last_check_completed_at_only_fills_an_empty_slot() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.seed_last_check_completed_at(Some(1_700_000_000_000));
        assert_eq!(service.last_check_completed_at(), Some(1_700_000_000_000));
        // Seeding again with a different value must not clobber a value
        // that was already restored.
        service.seed_last_check_completed_at(Some(1));
        assert_eq!(service.last_check_completed_at(), Some(1_700_000_000_000));
    }

    #[test]
    fn request_check_now_forces_an_immediate_check_after_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            let call = calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::NewerAvailable {
                release: manifest_with(if call == 0 { "2.0.0" } else { "3.0.0" }),
            }
        }));
        // Drive the initial steady-state check to completion so the
        // schedule has settled into the long-poll wait.
        drive_until_quiescent(&mut service);
        let baseline = calls.load(Ordering::SeqCst);
        // Manual request must force another check on the next tick even
        // though POLL_INTERVAL has not elapsed.
        service.request_check_now();
        service.tick(true);
        service.tick(true);
        assert!(
            calls.load(Ordering::SeqCst) > baseline,
            "manual request must trigger an extra fetch (baseline {baseline}, observed {})",
            calls.load(Ordering::SeqCst),
        );
        assert_eq!(
            service.state(),
            &UpdateState::Available {
                release: manifest_with("3.0.0")
            }
        );
    }

    #[test]
    fn request_check_now_forces_a_check_before_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            let call = calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::NewerAvailable {
                release: manifest_with(if call == 0 { "2.0.0" } else { "3.0.0" }),
            }
        }));

        service.request_check_now();
        service.tick(false);
        service.tick(false);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "manual request must not wait for settled idle"
        );
        assert_eq!(
            service.state(),
            &UpdateState::Available {
                release: manifest_with("2.0.0")
            }
        );
    }

    #[test]
    fn time_until_next_check_polls_while_discovery_is_in_flight() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| CheckOutcome::UpToDate));

        service.tick(true);

        assert_eq!(
            service.time_until_next_check(),
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn time_until_next_check_polls_until_settled_idle_latches() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());

        service.tick(false);

        assert_eq!(
            service.time_until_next_check(),
            Some(Duration::from_millis(250)),
            "startup discovery must keep ticking until settled idle can be observed"
        );
    }
    /// A bounded loopback HTTP peer. Each response records the request target.
    fn http_peer(
        responses: Vec<(String, Vec<u8>)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{BufRead, BufReader, Write};
        use std::net::{Ipv4Addr, TcpListener};
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let peer = std::thread::spawn(move || {
            let mut targets = Vec::new();
            for (headers, body) in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "HTTP request never arrived");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("accept: {error}"),
                    }
                };
                // Accepted sockets inherit the listener's nonblocking mode on macOS.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                targets.push(line.split_whitespace().nth(1).unwrap().to_owned());
                loop {
                    line.clear();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 {headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
            targets
        });
        (url, peer)
    }

    #[test]
    fn manifest_fetch_follows_redirect_and_rejects_non_success_status() {
        let (url, peer) = http_peer(vec![
            ("302 Found\r\nLocation: /signed/latest.json".into(), vec![]),
            ("200 OK".into(), b"signed manifest bytes\n".to_vec()),
            ("503 Service Unavailable".into(), b"unavailable".to_vec()),
        ]);
        assert_eq!(
            fetch_manifest(&format!("{url}/latest.json")).unwrap(),
            b"signed manifest bytes\n"
        );
        assert_eq!(
            fetch_manifest(&format!("{url}/unavailable")).unwrap_err(),
            "status: 503 Service Unavailable"
        );
        assert_eq!(
            peer.join().unwrap(),
            ["/latest.json", "/signed/latest.json", "/unavailable"]
        );
    }

    #[test]
    fn install_downloads_verifies_stages_payload_and_rejects_bad_hash() {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "SnappyRef/snappyref.exe",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"new executable bytes").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let sha = hex::encode(Sha256::digest(&bytes));
        let (url, peer) = http_peer(vec![
            ("200 OK".into(), bytes.clone()),
            ("200 OK".into(), bytes.clone()),
        ]);
        let root = TempRoot::new("install-handoff");
        for (version, hash) in [
            ("1.2.3", sha.as_str()),
            (
                "1.2.4",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ),
        ] {
            let (tx, rx) = channel();
            run_install(
                InstallContext {
                    version: version.into(),
                    url: format!("{url}/package.zip"),
                    sha256: hash.into(),
                    size: bytes.len() as u64,
                    install_root: root.to_path_buf(),
                    platform: TargetPlatform::Windows,
                },
                tx,
            );
            let events: Vec<_> = rx.into_iter().collect();
            let staging = install_paths::staging_dir(&root, version);
            if version == "1.2.3" {
                assert_eq!(
                    events.last(),
                    Some(&InstallProgress::Staged {
                        version: "1.2.3".into()
                    })
                );
                assert_eq!(
                    std::fs::read(staging.join("SnappyRef/snappyref.exe")).unwrap(),
                    b"new executable bytes"
                );
                assert_eq!(
                    std::fs::read_to_string(staging.join(".staged-sha256")).unwrap(),
                    sha
                );
                assert!(events.contains(&InstallProgress::Progress {
                    version: "1.2.3".into(),
                    percent: 100
                }));
            } else {
                assert_eq!(
                    events.last(),
                    Some(&InstallProgress::Failed {
                        reason: format!(
                            "sha256 mismatch: expected 0000000000000000000000000000000000000000000000000000000000000000, got {sha}"
                        ),
                    })
                );
                assert_eq!(std::fs::read_dir(root.join("updates")).unwrap().count(), 1);
                assert!(!staging.exists());
            }
            assert!(!install_paths::download_file_path(&root, version).exists());
        }
        assert_eq!(peer.join().unwrap(), ["/package.zip", "/package.zip"]);
    }

    #[test]
    fn discovery_preserves_the_active_install_and_both_staging_versions() {
        let root = TempRoot::new("in-flight-staging");
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".into(),
            install_root: Some(root.to_path_buf()),
            ..UpdateConfig::for_tests()
        });
        let mut releases = std::collections::VecDeque::from([
            CheckOutcome::NewerAvailable {
                release: manifest_with("2.0.0"),
            },
            CheckOutcome::NewerAvailable {
                release: manifest_with("3.0.0"),
            },
        ]);
        service.fetch_override = Some(Box::new(move || releases.pop_front().unwrap()));
        drive_until_quiescent(&mut service);
        let (started_tx, started_rx) = channel();
        service.install_override = Some(Box::new(move |context, progress| {
            started_tx.send((context, progress)).unwrap();
        }));
        service.start_install();
        let (context, _in_flight_worker) = started_rx.recv().unwrap();
        assert_eq!(context.version, "2.0.0");
        for (version, bytes) in [
            ("3.0.0", b"manifest".as_slice()),
            ("2.0.0", b"downloading".as_slice()),
            ("0.9.0", b"stale".as_slice()),
        ] {
            let dir = install_paths::staging_dir(&root, version);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("marker"), bytes).unwrap();
        }
        service.request_check_now();
        drive_until_quiescent(&mut service);
        assert_eq!(
            service.state(),
            &UpdateState::Downloading {
                version: "2.0.0".into(),
                percent: 0
            }
        );
        assert_eq!(
            std::fs::read(install_paths::staging_dir(&root, "3.0.0").join("marker")).unwrap(),
            b"manifest"
        );
        assert_eq!(
            std::fs::read(install_paths::staging_dir(&root, "2.0.0").join("marker")).unwrap(),
            b"downloading"
        );
        assert_eq!(std::fs::read_dir(root.join("updates")).unwrap().count(), 2);
        assert!(!install_paths::staging_dir(&root, "0.9.0").exists());
    }
}
