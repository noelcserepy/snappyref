//! PureRef `.pur` board exchange. Import turns a dropped file into a new
//! board; export writes the active board to a file. File work runs on a
//! background thread and reports back through [`PureRefExchange`].

mod export;
mod import;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

pub(crate) use export::{ExistingFile, ExportedPureRefBoard, PureRefExportPlan};
pub(crate) use import::ImportedPureRefBoard;

use crate::library::storage::Library;

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub(crate) fn is_pureref_board_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pur"))
}

/// Each result carries the job number its start returned, so a caller can
/// tell jobs apart.
pub(crate) enum PureRefExchangeResult {
    Imported(u64, Result<ImportedPureRefBoard, String>),
    Exported(u64, Result<ExportedPureRefBoard, String>),
}

pub(crate) struct PureRefExchange {
    sender: Sender<PureRefExchangeResult>,
    receiver: Receiver<PureRefExchangeResult>,
    jobs: u64,
}

impl Default for PureRefExchange {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            receiver,
            jobs: 0,
        }
    }
}

impl PureRefExchange {
    pub(crate) fn start_import(
        &mut self,
        ctx: &egui::Context,
        library: Library,
        path: PathBuf,
    ) -> u64 {
        self.spawn(ctx, move |job| {
            PureRefExchangeResult::Imported(job, import::import_board(&library, &path))
        })
    }

    pub(crate) fn start_export(
        &mut self,
        ctx: &egui::Context,
        plan: PureRefExportPlan,
        path: PathBuf,
        existing: ExistingFile,
    ) -> u64 {
        self.spawn(ctx, move |job| {
            PureRefExchangeResult::Exported(job, export::export_board(plan, path, existing))
        })
    }

    pub(crate) fn try_recv(&self) -> Option<PureRefExchangeResult> {
        self.receiver.try_recv().ok()
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(u64) -> PureRefExchangeResult + Send + 'static,
    ) -> u64 {
        self.jobs += 1;
        let job = self.jobs;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(work(job));
            ctx.request_repaint();
        });
        job
    }
}
