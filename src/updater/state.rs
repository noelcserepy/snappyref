//! The small `UpdateState` machine that drives the Update Pill.
//!
//! In this slice the pill has only three observable states:
//!
//! * `Hidden`   – no Available Update is known.
//! * `Available` – a verified manifest reports a version newer than the
//!   running app. The pill is shown.
//! * `Failed`   – the last check failed (network, signature, parse). The
//!   pill renders as a quiet failure indicator; ADR-0015 calls for no popup.
//!
//! Slice 5 will extend this with download/staging states. The transitions
//! here intentionally only cover what slice 4 surfaces.

/// Outcome of a single discovery attempt, fed to [`UpdateState::apply`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CheckOutcome {
    /// Manifest was fetched and verified; the version is greater than the
    /// running app.
    NewerAvailable {
        version: String,
        notes_url: String,
        release_page_url: String,
    },
    /// Manifest was fetched and verified, but its version does not outrank
    /// the running app.
    UpToDate,
    /// Discovery failed at any step (network, signature, parse, schema).
    Failed,
}

/// The pill state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateState {
    Hidden,
    Available {
        version: String,
        notes_url: String,
        release_page_url: String,
    },
    Failed,
}

impl UpdateState {
    pub(crate) fn apply(&mut self, outcome: CheckOutcome) {
        match outcome {
            CheckOutcome::NewerAvailable {
                version,
                notes_url,
                release_page_url,
            } => {
                *self = UpdateState::Available {
                    version,
                    notes_url,
                    release_page_url,
                };
            }
            CheckOutcome::UpToDate => {
                // Up-to-date never overwrites a previously-discovered
                // Available — a manifest server briefly serving an old
                // version must not retract the pill mid-session. The pill
                // also clears Failed when we recover.
                if matches!(self, UpdateState::Failed) {
                    *self = UpdateState::Hidden;
                }
            }
            CheckOutcome::Failed => {
                // A failure must not clobber an already-known Available
                // Update. ADR-0015: "the pill stays in its last known
                // state." Otherwise surface Failed.
                if matches!(self, UpdateState::Hidden) {
                    *self = UpdateState::Failed;
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn is_available(&self) -> bool {
        matches!(self, UpdateState::Available { .. })
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        UpdateState::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newer(version: &str) -> CheckOutcome {
        CheckOutcome::NewerAvailable {
            version: version.to_owned(),
            notes_url: "https://example/notes".to_owned(),
            release_page_url: "https://example/release".to_owned(),
        }
    }

    #[test]
    fn default_is_hidden() {
        assert_eq!(UpdateState::default(), UpdateState::Hidden);
    }

    #[test]
    fn hidden_to_available_on_newer() {
        let mut state = UpdateState::Hidden;
        state.apply(newer("1.2.3"));
        assert!(state.is_available());
    }

    #[test]
    fn hidden_stays_hidden_on_up_to_date() {
        let mut state = UpdateState::Hidden;
        state.apply(CheckOutcome::UpToDate);
        assert_eq!(state, UpdateState::Hidden);
    }

    #[test]
    fn hidden_to_failed_on_failure() {
        let mut state = UpdateState::Hidden;
        state.apply(CheckOutcome::Failed);
        assert_eq!(state, UpdateState::Failed);
    }

    #[test]
    fn failure_does_not_clobber_available() {
        let mut state = UpdateState::Hidden;
        state.apply(newer("1.2.3"));
        state.apply(CheckOutcome::Failed);
        assert!(
            state.is_available(),
            "Available must survive a transient failure"
        );
    }

    #[test]
    fn up_to_date_clears_failed() {
        let mut state = UpdateState::Failed;
        state.apply(CheckOutcome::UpToDate);
        assert_eq!(state, UpdateState::Hidden);
    }

    #[test]
    fn up_to_date_does_not_retract_available() {
        let mut state = UpdateState::Hidden;
        state.apply(newer("1.2.3"));
        state.apply(CheckOutcome::UpToDate);
        assert!(state.is_available());
    }

    #[test]
    fn available_can_be_replaced_with_newer_available() {
        let mut state = UpdateState::Hidden;
        state.apply(newer("1.2.3"));
        state.apply(newer("1.2.4"));
        match state {
            UpdateState::Available { version, .. } => assert_eq!(version, "1.2.4"),
            _ => panic!("expected Available"),
        }
    }
}
