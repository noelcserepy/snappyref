//! Undo/redo over the document history, including board lifecycle replay.

use eframe::egui;

use super::SnappyRefApp;

impl SnappyRefApp {
    pub(super) fn handle_history_shortcuts(&mut self, ctx: &egui::Context) {
        let undo_requested = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        let redo_requested = ctx.input(|input| {
            input.modifiers.command && input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        if redo_requested {
            self.redo_document_history(ctx);
        } else if undo_requested {
            self.undo_document_history(ctx);
        }
    }

    pub(super) fn undo_document_history(&mut self, ctx: &egui::Context) {
        match self.board_lifecycle().undo_document_history(ctx) {
            Ok(Some(status)) | Err(status) => self.status = status,
            Ok(None) => {}
        }
    }

    pub(super) fn redo_document_history(&mut self, ctx: &egui::Context) {
        match self.board_lifecycle().redo_document_history(ctx) {
            Ok(Some(status)) | Err(status) => self.status = status,
            Ok(None) => {}
        }
    }

    pub(super) fn record_pending_document_changes_history(&mut self, label: &str) {
        if let Err(error) = self.board_editing.record_pending_document_changes_history(
            &mut self.canvas,
            self.library.as_ref(),
            self.active_board_id,
            label,
        ) {
            self.status = error;
        }
    }
}
