//! Auto-update client discovery.
//!
//! This module hosts the discovery half of the auto-updater described in
//! ADR-0015. The runtime [`UpdateService`] runs on the egui update loop:
//!
//! 1. After the app reaches Settled Idle, it fetches `latest.json` from
//!    the configured manifest URL on a background thread so startup is
//!    never blocked.
//! 2. The response bytes are signature-verified against the public key
//!    embedded at compile time, then parsed.
//! 3. If the manifest version is greater than the running version, the
//!    pill enters Available; otherwise we stay Hidden.
//! 4. Failures back off exponentially up to 6 hours and never raise a
//!    popup; the pill state stays in its last known value.
//!
//! Download/staging belongs to slice 5. In slice 4 the "Install" button is
//! a stopgap that opens the release page in the user's default browser.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod descriptor;
pub(crate) mod discovery;
pub(crate) mod download;
pub(crate) mod install_paths;
pub(crate) mod manifest;
pub(crate) mod platform;
pub mod rollback;
pub(crate) mod stager;
pub(crate) mod state;
pub(crate) mod version;

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

/// Release page opened by the slice-4 stopgap Install button.
pub(crate) const DEFAULT_RELEASE_PAGE_URL: &str =
    "https://github.com/noelcserepy/snappyref-dist/releases/latest";

/// Network/parse timeout for the fetch worker. Keep small so a hung TCP
/// connection cannot keep a background thread alive past the next 6h tick.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Runtime state for the update service. Owned by the app and ticked from
/// the main update loop.
pub(crate) struct UpdateService {
    state: UpdateState,
    started_at: Instant,
    settled_idle_at: Option<Instant>,
    last_check: Option<Duration>,
    consecutive_failures: u32,
    in_flight: Option<Receiver<DiscoveryResult>>,
    next_check_after: Option<Duration>,
    config: UpdateConfig,
    /// Latest verified manifest, kept so a click on Install knows the URL
    /// and sha256 without re-fetching.
    last_manifest: Option<Manifest>,
    /// Wall-clock unix millis at which the most recent discovery check
    /// completed (success or failure). Settings Tab renders this; `None`
    /// before the first check returns.
    last_check_completed_at: Option<u64>,
    /// Set to true when [`request_check_now`] fires; consumed by the next
    /// tick to force a discovery cycle outside the steady-state schedule.
    manual_check_requested: bool,
    /// Install-worker progress receiver. `None` while no install is in
    /// flight. A finished install drops the channel via `Failed` or `Staged`.
    install_rx: Option<Receiver<InstallProgress>>,
    /// Optional override for tests: short-circuit the network and feed a
    /// canned outcome instead of spawning a thread.
    #[cfg(test)]
    test_fetch: Option<Box<dyn FnMut() -> DiscoveryResult + Send>>,
    /// Optional override for tests: feed install events directly.
    #[cfg(test)]
    test_install: Option<Box<dyn FnMut(InstallContext, Sender<InstallProgress>) + Send>>,
}

#[derive(Clone)]
pub(crate) struct UpdateConfig {
    pub manifest_url: String,
    pub release_page_url: String,
    pub current_version: String,
    /// Install root override, applied to the in-app updates/staging
    /// resolution. `None` resolves via `install_paths::resolve_install_root`.
    pub install_root: Option<PathBuf>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            manifest_url: DEFAULT_MANIFEST_URL.to_owned(),
            release_page_url: DEFAULT_RELEASE_PAGE_URL.to_owned(),
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
            install_root: None,
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
    pub platform: Option<TargetPlatform>,
}

/// Internal channel payload from the discovery worker. Carries both the
/// observable outcome and the parsed manifest so the service can store
/// the latter for use by the Install button.
#[derive(Clone, Debug)]
pub(crate) struct DiscoveryResult {
    pub outcome: CheckOutcome,
    pub manifest: Option<Manifest>,
}

impl UpdateService {
    pub(crate) fn new(config: UpdateConfig) -> Self {
        let service = Self {
            state: UpdateState::Hidden,
            started_at: Instant::now(),
            settled_idle_at: None,
            last_check: None,
            consecutive_failures: 0,
            in_flight: None,
            next_check_after: None,
            config,
            last_manifest: None,
            last_check_completed_at: None,
            manual_check_requested: false,
            install_rx: None,
            #[cfg(test)]
            test_fetch: None,
            #[cfg(test)]
            test_install: None,
        };
        // Best-effort startup hygiene: discard partial downloads from a
        // prior session so a half-written file cannot be served as a
        // verified update. Tests inject install_root via config.
        service.cleanup_partial_downloads();
        service
    }

    /// Build the pending-update descriptor for the currently-staged
    /// version, if the pill is in `Ready` state. Returns `None` if there
    /// is nothing to restart into, the host platform has no release
    /// shape, or the install root cannot be resolved.
    ///
    /// `launch_args` should be the argv tail (everything after argv[0])
    /// that the helper will replay on the new binary.
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
    /// Available update. Called by the popover's Install button.
    pub(crate) fn start_install(&mut self) {
        // Only allowed while we have a verified manifest and the pill
        // is showing Available.
        let manifest = match (&self.last_manifest, self.state.clone()) {
            (Some(manifest), UpdateState::Available { .. }) => manifest.clone(),
            _ => return,
        };
        let Some(platform) = TargetPlatform::current() else {
            // No platform entry for the running OS; the pill stays
            // Available so the user can still open the release page
            // manually via the popover's release-notes link.
            return;
        };
        let entry = manifest.entry_for(platform);
        let Some(install_root) = self.resolved_install_root() else {
            self.state = UpdateState::Failed;
            return;
        };
        let context = InstallContext {
            version: manifest.version.clone(),
            url: entry.url.clone(),
            sha256: entry.sha256.clone(),
            size: entry.size,
            install_root,
            platform: Some(platform),
        };
        self.state.begin_install(context.version.clone());
        let (tx, rx) = channel();
        self.install_rx = Some(rx);
        self.spawn_install(context, tx);
    }

    #[cfg(not(test))]
    fn spawn_install(&self, context: InstallContext, tx: Sender<InstallProgress>) {
        std::thread::spawn(move || run_install(context, tx));
    }

    #[cfg(test)]
    fn spawn_install(&mut self, context: InstallContext, tx: Sender<InstallProgress>) {
        if let Some(installer) = self.test_install.as_mut() {
            (installer)(context, tx);
        } else {
            let _ = tx.send(InstallProgress::Failed {
                reason: "no test installer".to_owned(),
            });
        }
    }

    /// Check whether a verified staged bundle for `manifest.version` is
    /// already on disk and, if so, mark the pill Ready directly. Called
    /// from the discovery hook so it runs once per verified manifest.
    fn detect_existing_staging(&mut self, manifest: &Manifest) {
        let Some(install_root) = self.resolved_install_root() else {
            return;
        };
        let Some(platform) = TargetPlatform::current() else {
            return;
        };
        let staging = install_paths::staging_dir(&install_root, &manifest.version);
        if !staging.is_dir() {
            return;
        }
        // Validate the staged layout matches what slice 6 will swap into
        // place. We only accept a structurally-valid bundle as Ready;
        // otherwise leave the pill alone.
        if validate_existing_staging(&staging, platform).is_ok() {
            self.state.mark_ready(manifest.version.clone());
        } else {
            // Stale or corrupt staging directory — clean it up so the
            // next download has a clean target.
            let _ = std::fs::remove_dir_all(&staging);
        }
    }

    pub(crate) fn state(&self) -> &UpdateState {
        &self.state
    }

    pub(crate) fn release_page_url(&self) -> &str {
        &self.config.release_page_url
    }

    /// Called once per egui frame. `app_settled_idle` should be true once
    /// the app's queues have drained for the first time since startup —
    /// the same signal the existing perf-scenario logic uses.
    pub(crate) fn tick(&mut self, app_settled_idle: bool) {
        if app_settled_idle && self.settled_idle_at.is_none() {
            self.settled_idle_at = Some(Instant::now());
        }

        if let Some(rx) = &self.in_flight {
            match rx.try_recv() {
                Ok(result) => {
                    self.in_flight = None;
                    self.last_check = Some(self.started_at.elapsed());
                    self.last_check_completed_at = current_unix_millis();
                    match &result.outcome {
                        CheckOutcome::Failed => self.consecutive_failures += 1,
                        CheckOutcome::NewerAvailable { .. } | CheckOutcome::UpToDate => {
                            self.consecutive_failures = 0;
                        }
                    }
                    if let Some(manifest) = result.manifest.clone() {
                        self.last_manifest = Some(manifest);
                    }
                    self.state.apply(result.outcome);
                    // On a confirmed new version, see if a previously
                    // staged bundle is already on disk for it and the
                    // pill can short-circuit to Ready.
                    if let Some(manifest) = self.last_manifest.clone() {
                        self.detect_existing_staging(&manifest);
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

        // Manual check (Settings Tab "Check for updates now") jumps the
        // queue regardless of the schedule. Throttling is the caller's
        // responsibility — the UI gates the button to one click per
        // minute so a wedged network does not get pinged repeatedly.
        if self.manual_check_requested && self.settled_idle_at.is_some() {
            self.manual_check_requested = false;
            self.start_check();
            return;
        }

        let input = ScheduleInput {
            settled_idle: self.settled_idle_at.is_some(),
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
    /// Tab "Check for updates now" button.
    pub(crate) fn request_check_now(&mut self) {
        self.manual_check_requested = true;
    }

    /// Unix-epoch milliseconds the most recent check completed. `None`
    /// before any check has resolved.
    pub(crate) fn last_check_completed_at(&self) -> Option<u64> {
        self.last_check_completed_at
    }

    /// Restore the persisted last-check timestamp on app launch. Called
    /// by [`crate::app::SnappyRefApp`] right after the User Session
    /// loads so the Settings Tab can render the timestamp before the
    /// first discovery cycle of this session completes.
    pub(crate) fn seed_last_check_completed_at(&mut self, unix_millis: Option<u64>) {
        if self.last_check_completed_at.is_none() {
            self.last_check_completed_at = unix_millis;
        }
    }

    /// Resolve `<install_root>` for the running process, honoring the
    /// same env override the discovery service uses. Returns `None` if
    /// resolution fails so callers can hide the Rollback button.
    pub(crate) fn install_root(&self) -> Option<PathBuf> {
        self.resolved_install_root()
    }

    /// Suggested wait until the next scheduled check, used by callers to
    /// request a future repaint. `None` when no check is scheduled (either
    /// pre-Settled-Idle or already in flight).
    pub(crate) fn time_until_next_check(&self) -> Option<Duration> {
        if self.in_flight.is_some() {
            return None;
        }
        self.next_check_after
    }

    fn start_check(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        let (tx, rx) = channel();
        self.spawn_fetch(tx);
        self.in_flight = Some(rx);
        self.next_check_after = None;
    }

    #[cfg(not(test))]
    fn spawn_fetch(&mut self, tx: Sender<DiscoveryResult>) {
        let config = self.config.clone();
        std::thread::spawn(move || {
            let outcome = run_fetch(&config);
            let _ = tx.send(outcome);
        });
    }

    #[cfg(test)]
    fn spawn_fetch(&mut self, tx: Sender<DiscoveryResult>) {
        if let Some(fetcher) = self.test_fetch.as_mut() {
            let outcome = (fetcher)();
            let _ = tx.send(outcome);
        } else {
            let _ = tx.send(DiscoveryResult {
                outcome: CheckOutcome::Failed,
                manifest: None,
            });
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
                    // as a failure so the pill is not stuck mid-progress.
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

/// Re-validate a staged bundle on disk using the same rules
/// [`stager::extract_and_validate`] applied when it was created. Returns
/// `Ok(())` only when the layout is still structurally valid.
fn validate_existing_staging(
    staging_root: &std::path::Path,
    platform: TargetPlatform,
) -> Result<(), String> {
    let bundle_root = staging_root.join(stager::EXPECTED_TOP_LEVEL);
    if !bundle_root.is_dir() {
        return Err("missing top-level SnappyRef/".to_owned());
    }
    match platform {
        TargetPlatform::Macos => {
            let macos_dir = bundle_root
                .join("SnappyRef.app")
                .join("Contents")
                .join("MacOS");
            if !macos_dir.is_dir() {
                return Err("missing Contents/MacOS".to_owned());
            }
            let any = std::fs::read_dir(&macos_dir)
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .any(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false));
            if !any {
                return Err("Contents/MacOS is empty".to_owned());
            }
        }
        TargetPlatform::Windows => {
            if !bundle_root.join("snappyref.exe").is_file() {
                return Err("missing snappyref.exe".to_owned());
            }
        }
    }
    Ok(())
}

/// Production install pipeline: download the zip, verify sha, extract
/// into staging, validate layout. Each phase reports progress through
/// `tx`. Drop semantics on the channel act as a failure signal too.
#[cfg(not(test))]
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
        // Throttle: forward only on a percent change or every >= 250ms,
        // whichever comes first. Guarantees > 1 update per second per
        // the issue's "updates >= 1/sec" requirement while keeping the
        // egui repaint cadence light.
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

    let Some(platform) = context.platform else {
        let _ = tx.send(InstallProgress::Failed {
            reason: "no release shape for this platform".to_owned(),
        });
        return;
    };

    let staging_root = install_paths::staging_dir(&context.install_root, &context.version);
    match stager::extract_and_validate(&zip_path, &staging_root, platform) {
        Ok(_) => {
            // Verified zip is no longer needed; remove it so the only
            // on-disk artifact for this version is the staging tree.
            let _ = std::fs::remove_file(&zip_path);
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

fn run_fetch(config: &UpdateConfig) -> DiscoveryResult {
    let bytes = match fetch_manifest(&config.manifest_url) {
        Ok(bytes) => bytes,
        Err(error) => {
            log_failure("fetch", &error);
            return DiscoveryResult {
                outcome: CheckOutcome::Failed,
                manifest: None,
            };
        }
    };
    let manifest = match manifest::parse_and_verify(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            log_failure("verify", &error);
            return DiscoveryResult {
                outcome: CheckOutcome::Failed,
                manifest: None,
            };
        }
    };
    let outcome = classify(&manifest, &config.current_version, &config.release_page_url);
    DiscoveryResult {
        outcome,
        manifest: Some(manifest),
    }
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
    // ADR-0015: failures must never raise a popup; logging only.
    eprintln!("update discovery {stage} failed: {error}");
}

/// Compare a verified manifest's version against `current_version` and
/// translate into the outcome the state machine consumes.
pub(crate) fn classify(
    manifest: &Manifest,
    current_version: &str,
    release_page_url: &str,
) -> CheckOutcome {
    if version::is_newer(current_version, &manifest.version) {
        CheckOutcome::NewerAvailable {
            version: manifest.version.clone(),
            notes_url: manifest.notes_url.clone(),
            release_page_url: release_page_url.to_owned(),
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
        match classify(&manifest, "0.1.0", "https://example/release") {
            CheckOutcome::NewerAvailable {
                version,
                notes_url,
                release_page_url,
            } => {
                assert_eq!(version, "9.9.9");
                assert_eq!(notes_url, "https://example/notes");
                assert_eq!(release_page_url, "https://example/release");
            }
            other => panic!("expected NewerAvailable, got {other:?}"),
        }
    }

    #[test]
    fn classify_emits_up_to_date_when_manifest_version_matches() {
        let bytes = build_manifest_bytes("0.1.0", &signing_key());
        let manifest = manifest::parse_and_verify_with_key(&bytes, &signing_key().verifying_key())
            .expect("test manifest verifies");
        assert_eq!(
            classify(&manifest, "0.1.0", "https://example/release"),
            CheckOutcome::UpToDate
        );
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
        let mut service = UpdateService::new(UpdateConfig::default());
        service.test_fetch = Some(Box::new(|| panic!("must not fetch before settled idle")));
        // Many frames before settled idle reports true. The service must
        // not touch the network or the state.
        for _ in 0..16 {
            service.tick(false);
        }
        assert_eq!(*service.state(), UpdateState::Hidden);
    }

    #[test]
    fn service_promotes_to_available_when_fetch_yields_newer() {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            release_page_url: "https://example/release".to_owned(),
            ..UpdateConfig::default()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.test_fetch = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            let outcome = classify(&manifest, "0.1.0", "https://example/release");
            DiscoveryResult {
                outcome,
                manifest: Some(manifest),
            }
        }));
        drive_until_quiescent(&mut service);
        match service.state() {
            UpdateState::Available {
                version,
                release_page_url,
                ..
            } => {
                assert_eq!(version, "1.0.0");
                assert_eq!(release_page_url, "https://example/release");
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn service_reports_failed_on_fetch_failure() {
        let mut service = UpdateService::new(UpdateConfig::default());
        service.test_fetch = Some(Box::new(|| DiscoveryResult {
            outcome: CheckOutcome::Failed,
            manifest: None,
        }));
        drive_until_quiescent(&mut service);
        assert_eq!(*service.state(), UpdateState::Failed);
        assert_eq!(service.consecutive_failures, 1);
    }

    #[test]
    fn failure_does_not_clobber_previously_available_state() {
        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            ..UpdateConfig::default()
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.test_fetch = Some(Box::new(move || {
            let n = calls_clone.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                DiscoveryResult {
                    outcome: CheckOutcome::NewerAvailable {
                        version: "9.9.9".to_owned(),
                        notes_url: "https://example/notes".to_owned(),
                        release_page_url: "https://example/release".to_owned(),
                    },
                    manifest: None,
                }
            } else {
                DiscoveryResult {
                    outcome: CheckOutcome::Failed,
                    manifest: None,
                }
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

    fn manifest_with(version: &str) -> Manifest {
        Manifest {
            version: version.to_owned(),
            notes_url: "https://example/notes".to_owned(),
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
    fn start_install_is_noop_when_no_manifest_is_cached() {
        let mut service = UpdateService::new(UpdateConfig::default());
        // No discovery has run yet, so last_manifest is None even if the
        // state happens to be Hidden. start_install must not transition.
        service.start_install();
        assert_eq!(*service.state(), UpdateState::Hidden);
    }

    #[test]
    fn start_install_is_noop_unless_state_is_available() {
        let mut service = UpdateService::new(UpdateConfig::default());
        service.last_manifest = Some(manifest_with("1.0.0"));
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
            ..UpdateConfig::default()
        });
        service.last_manifest = Some(manifest_with("1.0.0"));
        service.state.apply(CheckOutcome::NewerAvailable {
            version: "1.0.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        // Stub the install worker so start_install doesn't depend on
        // platform shape or filesystem.
        service.test_install = Some(Box::new(|_, _| {}));
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
        let mut service = UpdateService::new(UpdateConfig::default());
        service.last_manifest = Some(manifest_with("1.0.0"));
        service.state.apply(CheckOutcome::NewerAvailable {
            version: "1.0.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        service.config.install_root = Some(std::env::temp_dir());
        // Test installer immediately reports progress + Staged.
        service.test_install = Some(Box::new(|ctx, tx| {
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
        let mut service = UpdateService::new(UpdateConfig::default());
        service.last_manifest = Some(manifest_with("1.0.0"));
        service.state.apply(CheckOutcome::NewerAvailable {
            version: "1.0.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        service.config.install_root = Some(std::env::temp_dir());
        service.test_install = Some(Box::new(|_, tx| {
            let _ = tx.send(InstallProgress::Failed {
                reason: "boom".to_owned(),
            });
        }));
        service.start_install();
        service.tick(true);
        assert_eq!(*service.state(), UpdateState::Failed);
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
            ..UpdateConfig::default()
        });

        assert!(!partial.exists(), "partial download must be removed");
        assert!(staging.join("marker").exists(), "staging tree must survive");

        let _ = std::fs::remove_dir_all(&scratch);
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
        let staging = install_paths::staging_dir(&scratch, "1.0.0");
        match TargetPlatform::current().unwrap() {
            TargetPlatform::Macos => {
                let macos = staging
                    .join(stager::EXPECTED_TOP_LEVEL)
                    .join("SnappyRef.app")
                    .join("Contents")
                    .join("MacOS");
                std::fs::create_dir_all(&macos).unwrap();
                std::fs::write(macos.join("snappyref"), b"binary").unwrap();
            }
            TargetPlatform::Windows => {
                let bundle = staging.join(stager::EXPECTED_TOP_LEVEL);
                std::fs::create_dir_all(&bundle).unwrap();
                std::fs::write(bundle.join("snappyref.exe"), b"binary").unwrap();
            }
        }

        let mut service = UpdateService::new(UpdateConfig {
            current_version: "0.1.0".to_owned(),
            install_root: Some(scratch.clone()),
            ..UpdateConfig::default()
        });
        let key = signing_key();
        let bytes = build_manifest_bytes("1.0.0", &key);
        let public = key.verifying_key();
        let bytes_clone = bytes.clone();
        service.test_fetch = Some(Box::new(move || {
            let manifest = manifest::parse_and_verify_with_key(&bytes_clone, &public).unwrap();
            let outcome = classify(&manifest, "0.1.0", "https://example/release");
            DiscoveryResult {
                outcome,
                manifest: Some(manifest),
            }
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
    fn last_check_completed_at_records_a_timestamp_after_a_check_returns() {
        let mut service = UpdateService::new(UpdateConfig::default());
        service.test_fetch = Some(Box::new(|| DiscoveryResult {
            outcome: CheckOutcome::UpToDate,
            manifest: None,
        }));
        assert_eq!(service.last_check_completed_at(), None);
        drive_until_quiescent(&mut service);
        assert!(
            service.last_check_completed_at().is_some(),
            "last_check_completed_at must be set once a check resolves",
        );
    }

    #[test]
    fn seed_last_check_completed_at_only_fills_an_empty_slot() {
        let mut service = UpdateService::new(UpdateConfig::default());
        service.seed_last_check_completed_at(Some(1_700_000_000_000));
        assert_eq!(service.last_check_completed_at(), Some(1_700_000_000_000));
        // Seeding again with a different value must not clobber a value
        // that was already restored.
        service.seed_last_check_completed_at(Some(1));
        assert_eq!(service.last_check_completed_at(), Some(1_700_000_000_000));
    }

    #[test]
    fn request_check_now_forces_an_immediate_check_after_settled_idle() {
        let mut service = UpdateService::new(UpdateConfig::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = calls.clone();
        service.test_fetch = Some(Box::new(move || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            DiscoveryResult {
                outcome: CheckOutcome::UpToDate,
                manifest: None,
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
    }
}
