//! The small `UpdateState` machine behind the Settings update controls.
//!
//! The update UI has five observable states:
//!
//! * `Hidden`      – no Available Update is known.
//! * `Available`   – a verified manifest reports a version newer than
//!   the running app.
//! * `Downloading` – the user has clicked Install. The update UI renders
//!   percent-complete in place.
//! * `Ready`       – the downloaded bundle has been verified and staged.
//!   The Settings action offers Restart.
//! * `Failed`      – the last check, download, or staging step failed.
//!   Settings shows a quiet failure indicator; discovery/download/staging
//!   failures never raise a popup.

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

/// The update state rendered by Settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum UpdateState {
    #[default]
    Hidden,
    Available {
        version: String,
        notes_url: String,
        release_page_url: String,
    },
    /// User has clicked Install and the download is in flight. Percent is
    /// in `0..=100`; the version is captured so a manifest swap mid-flight
    /// cannot relabel the update UI.
    Downloading {
        version: String,
        percent: u8,
    },
    /// Download is verified and extracted into
    /// `<install_root>/updates/staging-{version}/`; Restart can now apply it.
    Ready {
        version: String,
    },
    Failed,
}

/// Progress events fired by the install worker; the service translates
/// them into state transitions on its loop tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstallProgress {
    /// Worker observed `percent` bytes complete for `version`.
    Progress { version: String, percent: u8 },
    /// Worker finished staging successfully.
    Staged { version: String },
    /// Worker hit a fatal error somewhere in download/verify/stage.
    Failed { reason: String },
}

impl UpdateState {
    pub(crate) fn apply(&mut self, outcome: CheckOutcome) {
        match outcome {
            CheckOutcome::NewerAvailable {
                version,
                notes_url,
                release_page_url,
            } => {
                // Discovery should never overwrite an in-flight download
                // or a ready bundle for the same or newer version — the
                // user is mid-install and the update UI must keep showing the
                // current progress/ready state.
                match self {
                    UpdateState::Downloading { version: v, .. }
                    | UpdateState::Ready { version: v }
                        if !crate::updater::version::is_newer_than(&version, v) =>
                    {
                        // Ignore: we already know about this version.
                    }
                    _ => {
                        *self = UpdateState::Available {
                            version,
                            notes_url,
                            release_page_url,
                        };
                    }
                }
            }
            CheckOutcome::UpToDate => {
                // Up-to-date never overwrites a previously-discovered
                // Available — a manifest server briefly serving an old
                // version must not retract the update UI mid-session. It also
                // never retracts Downloading or Ready. The update UI clears
                // Failed when we recover.
                if matches!(self, UpdateState::Failed) {
                    *self = UpdateState::Hidden;
                }
            }
            CheckOutcome::Failed => {
                // A failure must not clobber an already-known Available
                // Update, in-flight download, or staged bundle. The update
                // UI stays in its last known actionable state. Otherwise
                // surface Failed.
                if matches!(self, UpdateState::Hidden) {
                    *self = UpdateState::Failed;
                }
            }
        }
    }

    /// Begin the download phase for `version`. Called when the user clicks
    /// Install in Settings.
    pub(crate) fn begin_install(&mut self, version: String) {
        *self = UpdateState::Downloading {
            version,
            percent: 0,
        };
    }

    /// Apply a progress event from the install worker. Only relevant
    /// while the update UI is in `Downloading` for the same version; later
    /// progress on a stale download is silently ignored.
    pub(crate) fn apply_progress(&mut self, event: InstallProgress) {
        match event {
            InstallProgress::Progress { version, percent } => {
                if let UpdateState::Downloading {
                    version: current,
                    percent: current_pct,
                } = self
                    && *current == version
                {
                    *current_pct = percent.min(100);
                }
            }
            InstallProgress::Staged { version } => {
                if let UpdateState::Downloading {
                    version: current, ..
                } = self
                    && *current == version
                {
                    *self = UpdateState::Ready { version };
                }
            }
            InstallProgress::Failed { reason: _ } => {
                if matches!(self, UpdateState::Downloading { .. }) {
                    *self = UpdateState::Failed;
                }
            }
        }
    }

    /// Promote to Ready directly. Used at startup when a verified staged
    /// bundle is already on disk and the manifest still points at that
    /// version, per the download-staging spec.
    ///
    /// Refuses to clobber an in-flight `Downloading` whose version is the
    /// same or newer than what is being marked: the install worker owns
    /// the staging directory for that download and will emit `Staged`
    /// when extraction completes. Promoting mid-write would orphan the
    /// worker and could expose a partially-extracted bundle as Ready.
    pub(crate) fn mark_ready(&mut self, version: String) {
        if let UpdateState::Downloading {
            version: in_flight, ..
        } = self
            && !crate::updater::version::is_newer_than(&version, in_flight)
        {
            return;
        }
        *self = UpdateState::Ready { version };
    }

    #[cfg(test)]
    pub(crate) fn is_available(&self) -> bool {
        matches!(self, UpdateState::Available { .. })
    }

    #[cfg(test)]
    pub(crate) fn is_downloading(&self) -> bool {
        matches!(self, UpdateState::Downloading { .. })
    }

    #[cfg(test)]
    pub(crate) fn is_ready(&self) -> bool {
        matches!(self, UpdateState::Ready { .. })
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

    #[test]
    fn begin_install_moves_available_to_downloading_at_zero_percent() {
        let mut state = UpdateState::Hidden;
        state.apply(newer("1.2.3"));
        state.begin_install("1.2.3".to_owned());
        assert_eq!(
            state,
            UpdateState::Downloading {
                version: "1.2.3".to_owned(),
                percent: 0,
            }
        );
    }

    #[test]
    fn progress_updates_percent_during_download() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 0,
        };
        state.apply_progress(InstallProgress::Progress {
            version: "1.0.0".to_owned(),
            percent: 42,
        });
        assert_eq!(
            state,
            UpdateState::Downloading {
                version: "1.0.0".to_owned(),
                percent: 42,
            }
        );
    }

    #[test]
    fn progress_clamps_percent_to_100() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 0,
        };
        state.apply_progress(InstallProgress::Progress {
            version: "1.0.0".to_owned(),
            percent: 250,
        });
        assert!(matches!(
            state,
            UpdateState::Downloading { percent: 100, .. }
        ));
    }

    #[test]
    fn progress_for_different_version_is_ignored() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 50,
        };
        state.apply_progress(InstallProgress::Progress {
            version: "0.9.0".to_owned(),
            percent: 99,
        });
        assert!(matches!(
            state,
            UpdateState::Downloading { percent: 50, .. }
        ));
    }

    #[test]
    fn staged_event_promotes_downloading_to_ready() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 100,
        };
        state.apply_progress(InstallProgress::Staged {
            version: "1.0.0".to_owned(),
        });
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.0.0".to_owned()
            }
        );
    }

    #[test]
    fn failed_event_transitions_to_failed_only_when_downloading() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 30,
        };
        state.apply_progress(InstallProgress::Failed {
            reason: "boom".to_owned(),
        });
        assert_eq!(state, UpdateState::Failed);
    }

    #[test]
    fn failed_event_does_not_clobber_ready() {
        let mut state = UpdateState::Ready {
            version: "1.0.0".to_owned(),
        };
        state.apply_progress(InstallProgress::Failed {
            reason: "irrelevant".to_owned(),
        });
        assert!(state.is_ready());
    }

    #[test]
    fn discovery_does_not_clobber_in_flight_download_for_same_version() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 30,
        };
        state.apply(CheckOutcome::NewerAvailable {
            version: "1.0.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        assert!(state.is_downloading());
    }

    #[test]
    fn discovery_does_not_clobber_ready_for_same_version() {
        let mut state = UpdateState::Ready {
            version: "1.0.0".to_owned(),
        };
        state.apply(CheckOutcome::NewerAvailable {
            version: "1.0.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        assert!(state.is_ready());
    }

    #[test]
    fn discovery_replaces_ready_when_a_newer_version_appears() {
        let mut state = UpdateState::Ready {
            version: "1.0.0".to_owned(),
        };
        state.apply(CheckOutcome::NewerAvailable {
            version: "1.1.0".to_owned(),
            notes_url: "n".to_owned(),
            release_page_url: "r".to_owned(),
        });
        assert!(state.is_available());
    }

    #[test]
    fn mark_ready_promotes_directly_from_hidden() {
        // Used at startup when a verified staged bundle is already on disk
        // and discovery hasn't run yet.
        let mut state = UpdateState::Hidden;
        state.mark_ready("1.2.3".to_owned());
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.2.3".to_owned()
            }
        );
    }

    #[test]
    fn mark_ready_does_not_clobber_in_flight_download_for_same_version() {
        // A discovery cycle landing mid-download could see a structurally
        // valid (but in-flight) staging dir and try to promote it. The
        // install worker still owns transitions for this download.
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 30,
        };
        state.mark_ready("1.0.0".to_owned());
        assert!(state.is_downloading());
    }

    #[test]
    fn mark_ready_promotes_when_a_newer_version_is_already_staged() {
        // A newer manifest replaced an older Downloading via apply(); on
        // the next pass we may discover that the newer version is already
        // staged on disk. That promotion is legitimate.
        let mut state = UpdateState::Downloading {
            version: "1.0.0".to_owned(),
            percent: 30,
        };
        state.mark_ready("1.1.0".to_owned());
        assert!(state.is_ready());
    }
}
