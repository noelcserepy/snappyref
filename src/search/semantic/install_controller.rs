use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use crate::search::semantic::{LocalModelStoreStatus, SemanticSearchInstallProgress};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SemanticSearchInstallDisplay {
    Ready,
    Loading(String),
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SemanticSearchInstallSnapshot {
    pub(crate) model_store_status: LocalModelStoreStatus,
    pub(crate) display: SemanticSearchInstallDisplay,
    pub(crate) install_action_focus_requested: bool,
    pub(crate) status_message: String,
}

#[derive(Default)]
pub(crate) struct SemanticSearchInstallController {
    rx: Option<mpsc::Receiver<SemanticSearchInstallProgress>>,
    cancel: Option<Arc<AtomicBool>>,
    #[cfg(test)]
    test_installer: Option<Box<dyn FnMut(mpsc::Sender<SemanticSearchInstallProgress>)>>,
}

impl SemanticSearchInstallController {
    #[cfg(test)]
    pub(crate) fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    #[cfg(test)]
    pub(crate) fn set_test_installer(
        &mut self,
        installer: impl FnMut(mpsc::Sender<SemanticSearchInstallProgress>) + 'static,
    ) {
        self.test_installer = Some(Box::new(installer));
    }

    pub(crate) fn start(&mut self) -> Option<SemanticSearchInstallSnapshot> {
        if self.rx.is_some() {
            return None;
        }

        let (tx, rx) = mpsc::channel();
        let canceled = Arc::new(AtomicBool::new(false));
        self.rx = Some(rx);
        self.cancel = Some(canceled.clone());

        #[cfg(test)]
        if let Some(installer) = self.test_installer.as_mut() {
            (installer)(tx);
            return Some(installing_snapshot(0));
        }

        #[cfg(not(test))]
        {
            std::thread::spawn(move || {
                run_semantic_search_install_worker(tx, canceled);
            });
        }

        Some(installing_snapshot(0))
    }

    pub(crate) fn request_cancel(&mut self) -> Option<SemanticSearchInstallSnapshot> {
        let canceled = self.cancel.take()?;
        canceled.store(true, Ordering::Relaxed);
        self.rx = None;
        Some(snapshot_for_event(SemanticSearchInstallProgress::Canceled))
    }

    pub(crate) fn force_stop(&mut self) {
        if let Some(canceled) = self.cancel.take() {
            canceled.store(true, Ordering::Relaxed);
        }
        self.rx = None;
    }

    pub(crate) fn drain(&mut self) -> Option<SemanticSearchInstallSnapshot> {
        let rx = self.rx.take()?;

        let mut snapshot = None;
        let mut keep_rx = true;
        while let Ok(event) = rx.try_recv() {
            if matches!(
                event,
                SemanticSearchInstallProgress::Installed
                    | SemanticSearchInstallProgress::Failed { .. }
                    | SemanticSearchInstallProgress::Canceled
            ) {
                keep_rx = false;
                self.cancel = None;
            }
            snapshot = Some(snapshot_for_event(event));
        }

        if keep_rx {
            self.rx = Some(rx);
        }

        snapshot
    }
}

fn snapshot_for_event(event: SemanticSearchInstallProgress) -> SemanticSearchInstallSnapshot {
    match event {
        SemanticSearchInstallProgress::Progress { percent } => installing_snapshot(percent),
        SemanticSearchInstallProgress::Installed => SemanticSearchInstallSnapshot {
            model_store_status: LocalModelStoreStatus::Installed,
            display: SemanticSearchInstallDisplay::Ready,
            install_action_focus_requested: false,
            status_message: "Semantic Search installed".to_owned(),
        },
        SemanticSearchInstallProgress::Failed { error } => SemanticSearchInstallSnapshot {
            model_store_status: LocalModelStoreStatus::Failed(error.clone()),
            display: SemanticSearchInstallDisplay::Error(error.clone()),
            install_action_focus_requested: true,
            status_message: format!("Semantic Search install failed: {error}"),
        },
        SemanticSearchInstallProgress::Canceled => SemanticSearchInstallSnapshot {
            model_store_status: LocalModelStoreStatus::Missing,
            display: SemanticSearchInstallDisplay::Ready,
            install_action_focus_requested: true,
            status_message: "Semantic Search install canceled".to_owned(),
        },
    }
}

fn installing_snapshot(percent: u8) -> SemanticSearchInstallSnapshot {
    SemanticSearchInstallSnapshot {
        model_store_status: LocalModelStoreStatus::Downloading { percent },
        display: SemanticSearchInstallDisplay::Loading(format!(
            "Installing Semantic Search {percent}%"
        )),
        install_action_focus_requested: false,
        status_message: "Installing Semantic Search".to_owned(),
    }
}

#[cfg(not(test))]
fn run_semantic_search_install_worker(
    tx: mpsc::Sender<SemanticSearchInstallProgress>,
    canceled: Arc<AtomicBool>,
) {
    let data_root = match crate::local_data_location::resolve_data_root() {
        Ok(root) => root,
        Err(error) => {
            let _ = tx.send(SemanticSearchInstallProgress::Failed { error });
            return;
        }
    };
    let descriptor = crate::search::semantic::default_semantic_search_asset_descriptor();
    let _ = crate::search::semantic::install_semantic_search_asset(
        &data_root,
        &descriptor,
        &canceled,
        |progress| {
            let _ = tx.send(progress);
        },
    );
}
