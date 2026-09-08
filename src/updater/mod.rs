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

/// URL of the signed manifest published by CI to the `snappyref-dist`
/// release. The "latest" alias resolves to the most recent release asset.
pub(crate) const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/noelcserepy/snappyref-dist/releases/latest/download/latest.json";

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
                    self.state.apply(result);
                    self.write_diagnostic(&format!("state_after={:?}", self.state));
                    if let UpdateState::Available { release } = &self.state {
                        let release = release.clone();
                        self.detect_existing_staging(&release);
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
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    fn build_manifest_bytes(version: &str, key: &SigningKey) -> Vec<u8> {
        let body = json!({
            "channel": "stable",
            "version": version,
            "released_at": "2026-05-11T12:00:00Z",
            "notes_url": "https://example/notes",
            "platforms": {
                "macos": { "url": "https://example/mac.zip", "sha256": "0".repeat(64), "size": 1 },
                "windows": { "url": "https://example/win.zip", "sha256": "0".repeat(64), "size": 1 },
            },
        });
        let unsigned = serde_json::to_vec(&body).unwrap();
        let signature = key.sign(&unsigned);
        let mut signed = body;
        signed.as_object_mut().unwrap().insert(
            "signature".to_owned(),
            json!(hex::encode(signature.to_bytes())),
        );
        serde_json::to_vec(&signed).unwrap()
    }

    #[test]
    fn classify_emits_newer_when_manifest_version_is_higher() {
        let bytes = build_manifest_bytes("9.9.9", &signing_key());
        let manifest = manifest::parse_and_verify_with_key(&bytes, &signing_key().verifying_key())
            .expect("test manifest verifies");
        match classify(&manifest, "0.1.0") {
            CheckOutcome::NewerAvailable { release } => {
                assert_eq!(release.version, "9.9.9");
            }
            other => panic!("expected NewerAvailable, got {other:?}"),
        }
    }

    #[test]
    fn classify_emits_up_to_date_when_manifest_version_matches() {
        let bytes = build_manifest_bytes("0.1.0", &signing_key());
        let manifest = manifest::parse_and_verify_with_key(&bytes, &signing_key().verifying_key())
            .expect("test manifest verifies");
        assert_eq!(classify(&manifest, "0.1.0"), CheckOutcome::UpToDate);
    }

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
    fn service_stays_hidden_before_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| panic!("must not fetch before settled idle")));
        // Many frames before settled idle reports true. The service must
        // not touch the network or the state.
        for _ in 0..16 {
            service.tick(false);
        }
        assert_eq!(*service.state(), UpdateState::Hidden);
    }

    #[test]
    fn service_waits_through_long_startup_then_checks_once_at_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::UpToDate
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
    }

    #[test]
    fn service_promotes_to_available_when_fetch_yields_newer() {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "0.1.0")
        }));
        drive_until_quiescent(&mut service);
        match service.state() {
            UpdateState::Available { release } => {
                assert_eq!(release.version, "1.0.0");
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn service_reports_failed_on_fetch_failure() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| CheckOutcome::Failed));
        drive_until_quiescent(&mut service);
        assert!(matches!(service.state(), UpdateState::Failed { .. }));
        assert_eq!(service.consecutive_failures, 1);
    }

    #[test]
    fn failure_does_not_clobber_previously_available_state() {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            ..UpdateConfig::for_tests()
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            let n = calls_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                CheckOutcome::NewerAvailable {
                    release: manifest_with("9.9.9"),
                }
            } else {
                CheckOutcome::Failed
            }
        }));
        drive_until_quiescent(&mut service);
        assert!(service.state().is_available());
        // Force a second attempt by clearing the wait timer and bumping
        // time forward past the steady-state poll interval. The simplest
        // way: manually invoke start_check then tick to drain the second
        // outcome.
        service.last_check = Some(
            service
                .started_at
                .elapsed()
                .saturating_sub(discovery::POLL_INTERVAL),
        );
        // First tick recomputes schedule and starts the second check; the
        // synchronous test fetcher delivers the outcome before tick
        // returns; second tick drains it.
        service.tick(true);
        service.tick(true);
        assert!(
            service.state().is_available(),
            "Available state must survive a transient failure"
        );
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
            let root = crate::test_support::TempRoot::new("retained-release");
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
            service.install_override = Some(Box::new(move |context, _| {
                let expected = offered.entry_for(context.platform);
                assert_eq!(context.version, offered.version);
                assert_eq!(context.url, expected.url);
                assert_eq!(context.sha256, expected.sha256);
                assert_eq!(context.size, expected.size);
            }));
            service.start_install();
            assert!(service.state().is_downloading());
        }
    }

    #[test]
    fn preparation_failure_keeps_reason_and_requires_a_fresh_check() {
        let root = crate::test_support::TempRoot::new("failed-release-recheck");
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
        assert!(matches!(service.state(), UpdateState::Failed { .. }));
        service.request_check_now();
        drive_until_quiescent(&mut service);
        assert!(service.state().is_available());
    }

    #[test]
    fn start_install_is_noop_unless_state_is_available() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        // State is still Hidden. start_install must refuse to act so a
        // race between failure and click cannot kick off a phantom
        // download.
        service.start_install();
        assert_eq!(*service.state(), UpdateState::Hidden);
    }

    #[test]
    fn start_install_transitions_to_downloading_with_zero_percent() {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            ..UpdateConfig::for_tests()
        });
        service.state.apply(CheckOutcome::NewerAvailable {
            release: manifest_with("1.0.0"),
        });
        // Stub the install worker so start_install doesn't depend on
        // platform shape or filesystem.
        service.install_override = Some(Box::new(|_, _| {}));
        // Pin install root to a temp dir so resolve_install_root doesn't
        // try to derive it from the test binary's path.
        service.config.install_root = Some(std::env::temp_dir());
        service.start_install();
        match service.state() {
            UpdateState::Downloading { version, percent } => {
                assert_eq!(version, "1.0.0");
                assert_eq!(*percent, 0);
            }
            other => panic!("expected Downloading, got {other:?}"),
        }
    }

    #[test]
    fn drain_install_progress_promotes_to_ready_on_staged() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.state.apply(CheckOutcome::NewerAvailable {
            release: manifest_with("1.0.0"),
        });
        service.config.install_root = Some(std::env::temp_dir());
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
        assert!(
            service.state().is_ready(),
            "service should be Ready after Staged, got {:?}",
            service.state(),
        );
    }

    #[test]
    fn drain_install_progress_transitions_to_failed_on_install_error() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.state.apply(CheckOutcome::NewerAvailable {
            release: manifest_with("1.0.0"),
        });
        service.config.install_root = Some(std::env::temp_dir());
        service.install_override = Some(Box::new(|_, tx| {
            let _ = tx.send(InstallProgress::Failed {
                reason: "boom".to_owned(),
            });
        }));
        service.start_install();
        service.tick(true);
        assert!(matches!(service.state(), UpdateState::Failed { .. }));
    }

    #[test]
    fn cleanup_partial_downloads_removes_files_in_updates_dir() {
        // Simulate a partial zip left behind by a previous launch.
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let updates_dir = install_paths::updates_dir(&scratch);
        std::fs::create_dir_all(&updates_dir).unwrap();
        let partial = updates_dir.join("download-1.0.0.zip");
        std::fs::write(&partial, b"partial").unwrap();
        // And a staging dir that must NOT be touched.
        let staging = install_paths::staging_dir(&scratch, "0.9.0");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("marker"), b"keep").unwrap();

        let _service = UpdateService::new(UpdateConfig {
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });

        assert!(!partial.exists(), "partial download must be removed");
        assert!(staging.join("marker").exists(), "staging tree must survive");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// Create a structurally-valid staged bundle on disk and write the
    /// sha sidecar so detect_existing_staging accepts it. Returns the
    /// staging directory path so the caller can manipulate it further.
    fn write_valid_staging(install_root: &std::path::Path, version: &str, sha: &str) -> PathBuf {
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
        stager::record_source_sha(&staging, sha);
        staging
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn detect_existing_staging_rejects_missing_app_or_helper() {
        for executable in ["snappyref", "snappyref-updater"] {
            let root = crate::test_support::TempRoot::new("restart-missing-executable");
            let staging = write_valid_staging(&root, "9.9.9", &"0".repeat(64));
            std::fs::remove_file(staging.join(format!(
                "SnappyRef/SnappyRef.app/Contents/MacOS/{executable}"
            )))
            .unwrap();
            let mut service = UpdateService::new(UpdateConfig {
                install_root: Some(root.to_path_buf()),
                ..UpdateConfig::for_tests()
            });
            service.detect_existing_staging(&manifest_with("9.9.9"));
            assert!(
                !service.state().is_ready(),
                "missing {executable} became Ready"
            );
        }
    }

    #[test]
    fn detect_existing_staging_promotes_to_ready_when_manifest_matches() {
        // Set up an install root with a valid staged bundle for the
        // version the manifest will advertise. The platform-current()
        // function only resolves on macOS/Windows; on other hosts the
        // detect path early-returns, so we skip the assertion there.
        if TargetPlatform::current().is_none() {
            return;
        }
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-staged-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        // Manifest builder pins both platforms' sha to "0" repeated.
        write_valid_staging(&scratch, "1.0.0", &"0".repeat(64));

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "0.1.0")
        }));
        drive_until_quiescent(&mut service);
        assert!(
            service.state().is_ready(),
            "service should be Ready when a verified staged bundle is on disk for the manifest's version, got {:?}",
            service.state(),
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn detect_existing_staging_does_not_promote_when_manifest_is_not_newer() {
        if TargetPlatform::current().is_none() {
            return;
        }
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-up-to-date-staging-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        write_valid_staging(&scratch, "1.0.0", &"0".repeat(64));

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "1.0.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "1.0.0")
        }));
        drive_until_quiescent(&mut service);

        assert_eq!(*service.state(), UpdateState::Hidden);

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn detect_existing_staging_rejects_a_bundle_without_a_sha_sidecar() {
        // A staging dir produced by a pre-sidecar build (or one whose
        // sidecar was lost) must not be promoted: we can't prove it
        // matches the current manifest.
        if TargetPlatform::current().is_none() {
            return;
        }
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-no-sidecar-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let staging = write_valid_staging(&scratch, "1.0.0", &"0".repeat(64));
        // Strip the sidecar to simulate the pre-sidecar case.
        let _ = std::fs::remove_file(staging.join(stager::STAGED_SHA_FILE));

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "0.1.0")
        }));
        drive_until_quiescent(&mut service);
        assert!(
            service.state().is_available(),
            "service must stay Available when staging has no sidecar, got {:?}",
            service.state(),
        );
        assert!(!staging.exists(), "stale staging must be cleaned up");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn detect_existing_staging_rejects_a_bundle_whose_sidecar_sha_mismatches() {
        // Manifest republished with a different binary for the same
        // version: the staged bundle is now stale and must be rejected.
        if TargetPlatform::current().is_none() {
            return;
        }
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-stale-sha-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        // Manifest sha is "0".repeat(64); record a different one.
        let staging = write_valid_staging(&scratch, "1.0.0", &"f".repeat(64));

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "0.1.0")
        }));
        drive_until_quiescent(&mut service);
        assert!(
            service.state().is_available(),
            "service must stay Available when staging sha disagrees with manifest, got {:?}",
            service.state(),
        );
        assert!(!staging.exists(), "stale staging must be cleaned up");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn detect_existing_staging_sweeps_stale_versions_after_manifest_fetch() {
        // A staging-{older-version}/ from a prior session must not
        // accumulate forever. After we verify a manifest for the new
        // version, any non-matching staging dir is removed.
        if TargetPlatform::current().is_none() {
            return;
        }
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-updater-sweep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let stale = install_paths::staging_dir(&scratch, "0.9.0");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("debris"), b"old").unwrap();
        // The matching-version staging that should be kept.
        write_valid_staging(&scratch, "1.0.0", &"0".repeat(64));

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::for_tests()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.fetch_override = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            classify(&manifest, "0.1.0")
        }));
        drive_until_quiescent(&mut service);
        assert!(!stale.exists(), "stale staging-0.9.0/ must be removed");
        assert!(
            install_paths::staging_dir(&scratch, "1.0.0").exists(),
            "current-version staging must survive sweep",
        );

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn last_check_completed_at_records_a_timestamp_after_a_check_returns() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        service.fetch_override = Some(Box::new(|| CheckOutcome::UpToDate));
        assert_eq!(service.last_check_completed_at(), None);
        drive_until_quiescent(&mut service);
        assert!(
            service.last_check_completed_at().is_some(),
            "last_check_completed_at must be set once a check resolves",
        );
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
            calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::UpToDate
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
    }

    #[test]
    fn request_check_now_forces_a_check_before_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::for_tests());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.fetch_override = Some(Box::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            CheckOutcome::UpToDate
        }));

        service.request_check_now();
        service.tick(false);
        service.tick(false);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "manual request must not wait for settled idle"
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
}
