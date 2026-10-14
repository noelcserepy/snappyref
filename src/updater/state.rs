//! The small `UpdateState` machine behind the Settings update controls.
//!
//! The update UI has five observable states:
//!
//! * `Hidden`      – no Available Update is known.
//! * `Available`   – a verified manifest reports a version newer than
//!   the running app.
//! * `Downloading` – the user has clicked Update. The update UI renders
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
    NewerAvailable { release: super::manifest::Manifest },
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
        release: super::manifest::Manifest,
    },
    /// User has clicked Update and the download is in flight. Percent is
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
    Failed {
        reason: String,
        version: Option<String>,
    },
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
            CheckOutcome::NewerAvailable { release } => {
                let version = &release.version;
                // The install worker owns Downloading until its terminal event.
                // Discovery cannot relabel or orphan an active install.
                // Available and Ready retain the same or newer known release.
                match self {
                    UpdateState::Downloading { .. } => {}
                    UpdateState::Available {
                        release: super::manifest::Manifest { version: v, .. },
                    }
                    | UpdateState::Ready { version: v }
                        if !crate::updater::version::is_newer_than(version, v) =>
                    {
                        // Ignore: we already know about this version.
                    }
                    _ => {
                        *self = UpdateState::Available { release };
                    }
                }
            }
            CheckOutcome::UpToDate => {
                // Up-to-date never overwrites a previously-discovered
                // Available — a manifest server briefly serving an old
                // version must not retract the update UI mid-session. It also
                // never retracts Downloading or Ready. The update UI clears
                // Failed when we recover.
                if matches!(self, UpdateState::Failed { .. }) {
                    *self = UpdateState::Hidden;
                }
            }
            CheckOutcome::Failed => {
                // A failure must not clobber an already-known Available
                // Update, in-flight download, or staged bundle. The update
                // UI stays in its last known actionable state. Otherwise
                // surface Failed.
                if matches!(self, UpdateState::Hidden) {
                    *self = UpdateState::Failed {
                        reason: "Release discovery failed".to_owned(),
                        version: None,
                    };
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
            InstallProgress::Failed { reason } => {
                if let UpdateState::Downloading { version, .. } = self {
                    *self = UpdateState::Failed {
                        reason,
                        version: Some(version.clone()),
                    };
                }
            }
        }
    }

    /// Promote to Ready directly. Used at startup when a verified staged
    /// bundle is already on disk and the manifest still points at that
    /// version.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str) -> super::super::manifest::Manifest {
        super::super::tests::manifest_with(version)
    }

    #[test]
    fn discovery_retains_the_available_package_until_a_newer_release_arrives() {
        let mut state = UpdateState::Hidden;
        state.apply(CheckOutcome::NewerAvailable {
            release: release("1.2.3"),
        });
        assert_eq!(
            state,
            UpdateState::Available {
                release: release("1.2.3")
            }
        );
        for outcome in [
            CheckOutcome::Failed,
            CheckOutcome::UpToDate,
            CheckOutcome::NewerAvailable {
                release: release("1.2.2"),
            },
        ] {
            state.apply(outcome);
            assert_eq!(
                state,
                UpdateState::Available {
                    release: release("1.2.3")
                }
            );
        }
        state.apply(CheckOutcome::NewerAvailable {
            release: release("1.2.4"),
        });
        assert_eq!(
            state,
            UpdateState::Available {
                release: release("1.2.4")
            }
        );
    }

    #[test]
    fn discovery_failure_reports_its_reason_and_recovery_clears_it() {
        let mut state = UpdateState::Hidden;
        state.apply(CheckOutcome::UpToDate);
        assert_eq!(state, UpdateState::Hidden);
        state.apply(CheckOutcome::Failed);
        assert_eq!(
            state,
            UpdateState::Failed {
                reason: "Release discovery failed".into(),
                version: None
            }
        );
        state.apply(CheckOutcome::UpToDate);
        assert_eq!(state, UpdateState::Hidden);
    }

    #[test]
    fn install_progress_keeps_the_version_clamps_percent_and_finishes_ready() {
        let mut state = UpdateState::Hidden;
        state.begin_install("1.0.0".into());
        assert_eq!(
            state,
            UpdateState::Downloading {
                version: "1.0.0".into(),
                percent: 0
            }
        );
        state.apply_progress(InstallProgress::Progress {
            version: "1.0.0".into(),
            percent: 42,
        });
        assert_eq!(
            state,
            UpdateState::Downloading {
                version: "1.0.0".into(),
                percent: 42
            }
        );
        for event in [
            InstallProgress::Progress {
                version: "0.9.0".into(),
                percent: 99,
            },
            InstallProgress::Staged {
                version: "0.9.0".into(),
            },
        ] {
            state.apply_progress(event);
            assert_eq!(
                state,
                UpdateState::Downloading {
                    version: "1.0.0".into(),
                    percent: 42
                }
            );
        }
        state.apply_progress(InstallProgress::Progress {
            version: "1.0.0".into(),
            percent: 250,
        });
        assert_eq!(
            state,
            UpdateState::Downloading {
                version: "1.0.0".into(),
                percent: 100
            }
        );
        state.apply_progress(InstallProgress::Staged {
            version: "1.0.0".into(),
        });
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.0.0".into()
            }
        );
        state.apply_progress(InstallProgress::Failed {
            reason: "late failure".into(),
        });
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.0.0".into()
            }
        );
    }

    #[test]
    fn install_failure_retains_the_download_version_and_worker_reason() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".into(),
            percent: 30,
        };
        state.apply_progress(InstallProgress::Failed {
            reason: "Package checksum mismatch".into(),
        });
        assert_eq!(
            state,
            UpdateState::Failed {
                reason: "Package checksum mismatch".into(),
                version: Some("1.0.0".into())
            }
        );
    }

    #[test]
    fn discovery_and_existing_staging_preserve_a_download_until_a_newer_version_is_ready() {
        let mut state = UpdateState::Downloading {
            version: "1.0.0".into(),
            percent: 30,
        };
        for version in ["0.9.0", "1.0.0"] {
            state.apply(CheckOutcome::NewerAvailable {
                release: release(version),
            });
            state.mark_ready(version.into());
            assert_eq!(
                state,
                UpdateState::Downloading {
                    version: "1.0.0".into(),
                    percent: 30
                }
            );
        }
        state.mark_ready("1.1.0".into());
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.1.0".into()
            }
        );
        state.apply(CheckOutcome::NewerAvailable {
            release: release("1.1.0"),
        });
        assert_eq!(
            state,
            UpdateState::Ready {
                version: "1.1.0".into()
            }
        );
        state.apply(CheckOutcome::NewerAvailable {
            release: release("1.2.0"),
        });
        assert_eq!(
            state,
            UpdateState::Available {
                release: release("1.2.0")
            }
        );
        let mut hidden = UpdateState::Hidden;
        hidden.mark_ready("1.2.3".into());
        assert_eq!(
            hidden,
            UpdateState::Ready {
                version: "1.2.3".into()
            }
        );
    }
}
