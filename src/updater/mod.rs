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

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

pub(crate) mod discovery;
pub(crate) mod manifest;
pub(crate) mod state;
pub(crate) mod version;

pub(crate) use state::UpdateState;

use discovery::{NextAction, ScheduleInput, decide};
use manifest::Manifest;
use state::CheckOutcome;

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
    in_flight: Option<Receiver<CheckOutcome>>,
    next_check_after: Option<Duration>,
    config: UpdateConfig,
    /// Optional override for tests: short-circuit the network and feed a
    /// canned outcome instead of spawning a thread.
    #[cfg(test)]
    test_fetch: Option<Box<dyn FnMut() -> CheckOutcome + Send>>,
}

#[derive(Clone)]
pub(crate) struct UpdateConfig {
    pub manifest_url: String,
    pub release_page_url: String,
    pub current_version: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            manifest_url: DEFAULT_MANIFEST_URL.to_owned(),
            release_page_url: DEFAULT_RELEASE_PAGE_URL.to_owned(),
            current_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

impl UpdateService {
    pub(crate) fn new(config: UpdateConfig) -> Self {
        Self {
            state: UpdateState::Hidden,
            started_at: Instant::now(),
            settled_idle_at: None,
            last_check: None,
            consecutive_failures: 0,
            in_flight: None,
            next_check_after: None,
            config,
            #[cfg(test)]
            test_fetch: None,
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
                Ok(outcome) => {
                    self.in_flight = None;
                    self.last_check = Some(self.started_at.elapsed());
                    match &outcome {
                        CheckOutcome::Failed => self.consecutive_failures += 1,
                        CheckOutcome::NewerAvailable { .. } | CheckOutcome::UpToDate => {
                            self.consecutive_failures = 0;
                        }
                    }
                    self.state.apply(outcome);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.in_flight = None;
                    self.last_check = Some(self.started_at.elapsed());
                    self.consecutive_failures += 1;
                    self.state.apply(CheckOutcome::Failed);
                }
            }
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
    fn spawn_fetch(&mut self, tx: Sender<CheckOutcome>) {
        let config = self.config.clone();
        std::thread::spawn(move || {
            let outcome = run_fetch(&config);
            let _ = tx.send(outcome);
        });
    }

    #[cfg(test)]
    fn spawn_fetch(&mut self, tx: Sender<CheckOutcome>) {
        if let Some(fetcher) = self.test_fetch.as_mut() {
            let outcome = (fetcher)();
            let _ = tx.send(outcome);
        } else {
            let _ = tx.send(CheckOutcome::Failed);
        }
    }
}

fn run_fetch(config: &UpdateConfig) -> CheckOutcome {
    let bytes = match fetch_manifest(&config.manifest_url) {
        Ok(bytes) => bytes,
        Err(error) => {
            log_failure("fetch", &error);
            return CheckOutcome::Failed;
        }
    };
    let manifest = match manifest::parse_and_verify(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            log_failure("verify", &error);
            return CheckOutcome::Failed;
        }
    };
    classify(&manifest, &config.current_version, &config.release_page_url)
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
            classify(&manifest, "0.1.0", "https://example/release")
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
        service.test_fetch = Some(Box::new(|| CheckOutcome::Failed));
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
                CheckOutcome::NewerAvailable {
                    version: "9.9.9".to_owned(),
                    notes_url: "https://example/notes".to_owned(),
                    release_page_url: "https://example/release".to_owned(),
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
}
