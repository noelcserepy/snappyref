//! PureRef `.pur` board exchange. Import turns a dropped file into a new
//! board; export writes the active board to a file. File work runs on a
//! background thread and reports back through [`PureRefExchange`].

mod export;
mod import;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

pub(crate) use export::{ExportedPureRefBoard, PureRefExportPlan};
pub(crate) use import::ImportedPureRefBoard;

use crate::library::storage::Library;

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub(crate) fn is_pureref_board_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pur"))
}

/// Each result names the file it worked on, so a caller can tell jobs apart.
pub(crate) enum PureRefExchangeResult {
    Imported(PathBuf, Result<ImportedPureRefBoard, String>),
    Exported(PathBuf, Result<ExportedPureRefBoard, String>),
}

pub(crate) struct PureRefExchange {
    sender: Sender<PureRefExchangeResult>,
    receiver: Receiver<PureRefExchangeResult>,
}

impl Default for PureRefExchange {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self { sender, receiver }
    }
}

impl PureRefExchange {
    pub(crate) fn start_import(&self, ctx: &egui::Context, library: Library, path: PathBuf) {
        self.spawn(ctx, move || {
            let imported = import::import_board(&library, &path);
            PureRefExchangeResult::Imported(path, imported)
        });
    }

    pub(crate) fn start_export(&self, ctx: &egui::Context, plan: PureRefExportPlan, path: PathBuf) {
        self.spawn(ctx, move || {
            let exported = export::export_board(plan, path.clone());
            PureRefExchangeResult::Exported(path, exported)
        });
    }

    pub(crate) fn try_recv(&self) -> Option<PureRefExchangeResult> {
        self.receiver.try_recv().ok()
    }

    fn spawn(
        &self,
        ctx: &egui::Context,
        work: impl FnOnce() -> PureRefExchangeResult + Send + 'static,
    ) {
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(work());
            ctx.request_repaint();
        });
    }
}
