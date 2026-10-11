//! PureRef `.pur` import and export, wired to the active board.

use std::path::PathBuf;

use eframe::egui;

use crate::data::{BoardDocumentSnapshot, CanvasAnnotationGeometry, CanvasAnnotationKind};
use crate::pureref_exchange::{ImportedPureRefBoard, PureRefExchangeResult, PureRefExportPlan};

use super::SnappyRefApp;

impl SnappyRefApp {
    pub(super) fn start_pureref_import(&mut self, ctx: &egui::Context, path: PathBuf) {
        let Some(library) = &self.library else {
            self.status = "Storage unavailable".to_owned();
            return;
        };
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.status = format!("Importing PureRef board {stem}…");
        self.pureref_exchange
            .start_import(ctx, library.clone(), path);
    }

    pub(super) fn export_pureref(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let plan = PureRefExportPlan::from_canvas(&self.canvas);
        if plan.is_empty() {
            self.status = plan.empty_status();
            return;
        }
        let board_name = self
            .board_summaries()
            .iter()
            .find(|summary| summary.board.id == self.active_board_id)
            .map(|summary| summary.board.name.clone())
            .unwrap_or_else(|| "Board".to_owned());
        let Some(path) = (self.pureref_save_path)(frame, &format!("{board_name}.pur")) else {
            return;
        };
        self.status = format!("Exporting {board_name} to PureRef…");
        self.pureref_exchange.start_export(ctx, plan, path);
    }

    pub(super) fn handle_pureref_exchange_results(&mut self, ctx: &egui::Context) {
        while let Some(result) = self.pureref_exchange.try_recv() {
            let (path, outcome) = match result {
                PureRefExchangeResult::Imported(path, imported) => (
                    path,
                    imported
                        .map_err(|error| format!("PureRef import failed: {error}"))
                        .and_then(|imported| self.open_imported_pureref_board(ctx, imported)),
                ),
                PureRefExchangeResult::Exported(path, exported) => (
                    path,
                    exported
                        .map(|exported| exported.status())
                        .map_err(|error| format!("PureRef export failed: {error}")),
                ),
            };
            self.status = outcome.clone().unwrap_or_else(|error| error);
            self.answer_pureref_control_call(&path, outcome);
        }
    }

    fn open_imported_pureref_board(
        &mut self,
        ctx: &egui::Context,
        imported: ImportedPureRefBoard,
    ) -> Result<String, String> {
        let status = imported.status();
        let mut document = imported.document;
        // Labels take their size from their text, which decides the framing.
        let fit_textboxes = |document: &mut BoardDocumentSnapshot| {
            let mut bounds = imported.image_bounds;
            for textbox in &mut document.annotations {
                let size =
                    crate::canvas::textbox_size_for_text(ctx, &textbox.text, textbox.text_size);
                let center = egui::Rect::from_two_pos(textbox.start(), textbox.end()).center();
                let rect = egui::Rect::from_center_size(center, size);
                textbox.geometry = CanvasAnnotationGeometry::from_kind(
                    CanvasAnnotationKind::Textbox,
                    rect.min,
                    rect.max,
                );
                bounds = Some(bounds.map_or(rect, |bounds| bounds.union(rect)));
            }
            bounds
        };
        if let Some(bounds) = fit_textboxes(&mut document) {
            let screen_rect = ctx.viewport_rect();
            let (pan, zoom) = crate::canvas::focus_framing(
                screen_rect,
                self.visible_canvas_screen_rect(screen_rect),
                bounds,
            );
            document.viewport.camera_pan_x = pan.x;
            document.viewport.camera_pan_y = pan.y;
            document.viewport.camera_zoom = zoom;
        }
        self.board_lifecycle()
            .create_from_document(document)
            .map(|()| status)
    }
}
