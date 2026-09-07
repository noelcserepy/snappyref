//! Undo/redo over the document history, including board lifecycle replay.

use eframe::egui;

use crate::active_board::history::{
    BoardLifecycleHistoryAction, HistoryAction, HistoryChange, HistoryDirection,
};

use super::HistoryCanvasReplay;
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
        if self.library.is_none() {
            self.status = "Storage unavailable".to_owned();
            return;
        }
        let owner_before = self.active_board_id;
        let Some(applied) = self.document_history.undo(owner_before) else {
            return;
        };
        let owner_after = applied
            .action
            .owner_after_replay(owner_before, applied.direction);
        self.document_history
            .reassign_applied(owner_before, owner_after, applied.direction);
        if let Err(error) = self.apply_history_action(ctx, &applied.action, applied.direction) {
            self.status = error;
            return;
        }
        self.status = format!("Undid {}", applied.action.label());
    }

    pub(super) fn redo_document_history(&mut self, ctx: &egui::Context) {
        if self.library.is_none() {
            self.status = "Storage unavailable".to_owned();
            return;
        }
        let owner_before = self.active_board_id;
        let Some(applied) = self.document_history.redo(owner_before) else {
            return;
        };
        let owner_after = applied
            .action
            .owner_after_replay(owner_before, applied.direction);
        self.document_history
            .reassign_applied(owner_before, owner_after, applied.direction);
        if let Err(error) = self.apply_history_action(ctx, &applied.action, applied.direction) {
            self.status = error;
            return;
        }
        self.status = format!("Redid {}", applied.action.label());
    }

    pub(super) fn apply_history_action(
        &mut self,
        ctx: &egui::Context,
        action: &HistoryAction,
        direction: HistoryDirection,
    ) -> Result<(), String> {
        match action {
            HistoryAction::Document(document_action) => {
                let replay = self.apply_history_to_active_canvas(document_action, direction);
                self.document_persistence.enqueue_document_unit(
                    self.active_board_id,
                    replay.document_changes,
                    &self.document_history,
                )?;
                self.load_history_placement_content(ctx, replay.placements_needing_content)
            }
            HistoryAction::Board(lifecycle_action) => {
                self.apply_board_lifecycle_history(ctx, lifecycle_action, direction)
            }
        }
    }

    /// The Board document as accepted so far: durable rows (or the accepted
    /// creation while the Board is not durable yet) with queued history
    /// applied on top, so opening a Board never shows state that durability
    /// still lags behind.
    pub(super) fn accepted_board_document(
        &self,
        library: &crate::library::storage::Library,
        board_id: i64,
    ) -> Result<crate::data::BoardDocumentSnapshot, String> {
        let mut document = match library.load_board_document(board_id) {
            Ok(mut document) => {
                self.document_history
                    .apply_accepted_document_state(board_id, &mut document);
                document
            }
            Err(error) => self
                .document_history
                .accepted_document_for_created_board(board_id)
                .ok_or(error)?,
        };
        if let Some(board) = self
            .board_summaries()
            .into_iter()
            .find(|summary| summary.board.id == board_id)
            .map(|summary| summary.board)
        {
            document.board = board;
        }
        Ok(document)
    }

    pub(super) fn apply_board_lifecycle_history(
        &mut self,
        ctx: &egui::Context,
        action: &BoardLifecycleHistoryAction,
        direction: HistoryDirection,
    ) -> Result<(), String> {
        let Some(library) = self.library.clone() else {
            return Err("Storage unavailable".to_owned());
        };
        let active_before_replay = self.active_board_id;
        match (action, direction) {
            (
                BoardLifecycleHistoryAction::Created {
                    document,
                    active_before,
                },
                HistoryDirection::Backward,
            ) => {
                let active_document = self.accepted_board_document(&library, *active_before)?;
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                        board_id: document.board.id,
                        active_board_id: *active_before,
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.archive(document.board.id);
                self.open_board_document_snapshot(ctx, &active_document)?;
            }
            (BoardLifecycleHistoryAction::Created { document, .. }, HistoryDirection::Forward) => {
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Restore {
                        document: document.clone(),
                        active_board_id: document.board.id,
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.restore_document(document);
                self.open_board_document_snapshot(ctx, document)?;
            }
            (
                BoardLifecycleHistoryAction::Renamed {
                    board_id,
                    before_name,
                    ..
                },
                HistoryDirection::Backward,
            ) => {
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                        board_id: *board_id,
                        name: before_name.clone(),
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.rename(*board_id, before_name);
            }
            (
                BoardLifecycleHistoryAction::Renamed {
                    board_id,
                    after_name,
                    ..
                },
                HistoryDirection::Forward,
            ) => {
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                        board_id: *board_id,
                        name: after_name.clone(),
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.rename(*board_id, after_name);
            }
            (
                BoardLifecycleHistoryAction::Deleted {
                    document,
                    active_before,
                    ..
                },
                HistoryDirection::Backward,
            ) => {
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Restore {
                        document: document.clone(),
                        active_board_id: *active_before,
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.restore_document(document);
                if active_before_replay != *active_before {
                    self.open_board_document_snapshot(ctx, document)?;
                }
            }
            (
                BoardLifecycleHistoryAction::Deleted {
                    document,
                    active_after,
                    ..
                },
                HistoryDirection::Forward,
            ) => {
                let active_document = if active_before_replay != *active_after {
                    Some(self.accepted_board_document(&library, *active_after)?)
                } else {
                    None
                };
                self.document_persistence.apply_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                        board_id: document.board.id,
                        active_board_id: *active_after,
                    },
                    &self.document_history,
                )?;
                self.board_summaries_cache.archive(document.board.id);
                if let Some(active_document) = active_document {
                    self.open_board_document_snapshot(ctx, &active_document)?;
                }
            }
        }

        Ok(())
    }

    pub(super) fn open_board_document_snapshot(
        &mut self,
        ctx: &egui::Context,
        document: &crate::data::BoardDocumentSnapshot,
    ) -> Result<(), String> {
        self.clear_active_board_transients();
        self.canvas.clear_for_board_switch();
        self.active_board_id = document.board.id;
        self.canvas.apply_viewport_state(&document.viewport);
        self.canvas.restore_groups(document.groups.clone());
        for placement in &document.placements {
            self.canvas.add_unresolved_placement(placement.clone());
        }
        for annotation in &document.annotations {
            self.canvas.add_annotation(annotation.clone());
        }
        self.canvas
            .restore_canvas_stack_order(document.stack_order.clone());
        self.load_history_placement_content(ctx, document.placements.clone())?;
        self.save_user_session();
        Ok(())
    }

    pub(super) fn apply_history_to_active_canvas(
        &mut self,
        action: &crate::active_board::history::DocumentHistoryAction,
        direction: HistoryDirection,
    ) -> HistoryCanvasReplay {
        let mut document_changes = crate::canvas::CanvasDocumentChanges::default();
        let mut placements_needing_content = Vec::new();
        for change in &action.annotations {
            let Some(target) = change.annotation_target(direction) else {
                continue;
            };
            self.canvas
                .apply_annotation_history_state(Some(target.clone()));
            document_changes.updated_annotations.push(target);
        }

        for change in &action.placements {
            let Some(target) = change.placement_target(direction) else {
                continue;
            };
            self.content_resolution
                .invalidate_placement(target.state.id);
            if let Some(placement) = self
                .canvas
                .apply_placement_history_state(Some(target.clone()))
            {
                placements_needing_content.push(placement);
            }
            document_changes.updated_placements.push(target.state);
        }

        if let Some(groups) = action
            .groups
            .as_ref()
            .and_then(|change| change.target(direction))
        {
            self.canvas.restore_groups(groups.clone());
            document_changes.groups = Some(groups);
        }
        if let Some(change) = &action.stack_order {
            let target = change.target(direction);
            if let Some(stack_order) = target {
                self.canvas.restore_canvas_stack_order(stack_order.clone());
                document_changes.stack_order = Some(stack_order);
            }
        }
        HistoryCanvasReplay {
            document_changes,
            placements_needing_content,
        }
    }

    pub(super) fn load_history_placement_content(
        &mut self,
        ctx: &egui::Context,
        placements: Vec<crate::data::Placement>,
    ) -> Result<(), String> {
        let Some(library) = self.library.clone() else {
            return Err("Storage unavailable".to_owned());
        };
        for placement in placements {
            if let Some(import_worker) = &self.import_worker {
                let reservation = self
                    .content_resolution
                    .reserve_history_placement(self.active_board_id, placement.state.id);
                if let Err(error) =
                    import_worker.enqueue(crate::jobs::ImagePrepRequest::LoadExisting {
                        target_board_id: self.active_board_id,
                        queue_id: Some(reservation.queue_id),
                        placement,
                        max_texture_side: self.max_texture_side,
                        canvas_image_settings: crate::jobs::CanvasImageSettings {
                            max_side: self.settings.canvas_image_max_side,
                        },
                        target_lod: crate::data::CanvasImageLod::Working,
                        priority: crate::jobs::ExistingLoadPriority::Background,
                        trace: None,
                    })
                {
                    self.content_resolution.cancel(reservation);
                    return Err(error);
                }
                continue;
            }
            match crate::jobs::load_existing(
                &library,
                self.active_board_id,
                None,
                placement,
                self.max_texture_side,
                crate::jobs::CanvasImageSettings {
                    max_side: self.settings.canvas_image_max_side,
                },
                crate::data::CanvasImageLod::Working,
                None,
                self.wgpu_render_state.as_ref(),
            ) {
                crate::jobs::ImagePrepResult::Loaded(loaded) => {
                    self.integrate_loaded_import(ctx, loaded);
                }
                crate::jobs::ImagePrepResult::Failed { error, .. } => return Err(error),
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn record_stack_order_history(
        &mut self,
        label: &str,
        before_stack_order: Vec<crate::data::CanvasStackItem>,
    ) {
        self.document_history.record_applied(
            self.active_board_id,
            HistoryAction::document(
                label.to_owned(),
                Vec::new(),
                Vec::new(),
                Some(HistoryChange {
                    before: Some(before_stack_order),
                    after: Some(self.canvas.canvas_stack_order()),
                }),
            ),
        );
        self.persist_recorded_canvas_changes();
    }

    pub(super) fn record_created_selection_history(
        &mut self,
        label: String,
        before_stack_order: Vec<crate::data::CanvasStackItem>,
    ) {
        let placement_changes = self
            .canvas
            .selected_placement_ids()
            .into_iter()
            .filter_map(|placement_id| self.canvas.placement_for_id(placement_id))
            .map(|placement| HistoryChange {
                before: None,
                after: Some(placement),
            })
            .collect();
        let annotation_changes = self
            .canvas
            .selected_annotation_ids()
            .into_iter()
            .filter_map(|annotation_id| self.canvas.annotation_for_id(annotation_id))
            .map(|annotation| HistoryChange {
                before: None,
                after: Some(annotation),
            })
            .collect();
        let groups = self
            .canvas
            .pending_document_changes_preview()
            .groups
            .map(|after| HistoryChange {
                before: self
                    .document_history
                    .accepted_groups(self.active_board_id)
                    .or_else(|| {
                        self.library
                            .as_ref()
                            .and_then(|library| library.load_groups().ok())
                    }),
                after: Some(after),
            });
        self.document_history.record_applied(
            self.active_board_id,
            HistoryAction::document(
                label,
                placement_changes,
                annotation_changes,
                Some(HistoryChange {
                    before: Some(before_stack_order),
                    after: Some(self.canvas.canvas_stack_order()),
                }),
            )
            .with_groups(groups),
        );
        self.persist_recorded_canvas_changes();
    }

    pub(super) fn record_pending_document_changes_history(&mut self, label: &str) {
        self.canvas.reflow_dirty_group_members();
        let Some(library) = &self.library else {
            return;
        };
        let changes = self.canvas.pending_document_changes_preview();
        if !changes.has_changes() {
            return;
        }

        let current_items = library.load_placements().unwrap_or_default();
        let current_annotations = library.load_canvas_annotations().unwrap_or_default();
        let current_stack_order = library.load_canvas_stack_order().ok();
        let placement_changes = changes
            .updated_placements
            .into_iter()
            .filter_map(|after| {
                let before = self
                    .document_history
                    .accepted_placement(self.active_board_id, after.id)
                    .or_else(|| {
                        current_items
                            .iter()
                            .find(|placement| placement.state.id == after.id)
                            .cloned()
                    });
                let mut placement = before
                    .as_ref()
                    .cloned()
                    .or_else(|| self.canvas.placement_for_id(after.id))?;
                placement.state = after;
                Some(HistoryChange {
                    before,
                    after: Some(placement),
                })
            })
            .collect::<Vec<_>>();
        let annotation_changes = changes
            .updated_annotations
            .into_iter()
            .map(|after| {
                let before = self
                    .document_history
                    .accepted_annotation(self.active_board_id, after.id)
                    .or_else(|| {
                        current_annotations
                            .iter()
                            .find(|annotation| annotation.id == after.id)
                            .cloned()
                    });
                HistoryChange {
                    before,
                    after: Some(after),
                }
            })
            .collect::<Vec<_>>();
        let stack_order = changes.stack_order.map(|after| HistoryChange {
            before: self
                .document_history
                .accepted_stack_order(self.active_board_id)
                .or(current_stack_order),
            after: Some(after),
        });

        let groups = changes.groups.map(|after| HistoryChange {
            before: self
                .document_history
                .accepted_groups(self.active_board_id)
                .or_else(|| library.load_groups().ok()),
            after: Some(after),
        });
        self.document_history.record_applied(
            self.active_board_id,
            HistoryAction::document(
                label.to_owned(),
                placement_changes,
                annotation_changes,
                stack_order,
            )
            .with_groups(groups),
        );
    }
}
