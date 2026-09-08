//! Board activation and lifecycle replay complete through one concrete owner.
use super::editing::BoardEditing;
use crate::active_board::history::{BoardLifecycleHistoryAction, HistoryAction, HistoryDirection};
use crate::data::{BoardDocumentSnapshot, UserSession};
use crate::library::storage::Library;
use eframe::egui;

pub(crate) struct BoardLifecycle<'a> {
    pub(crate) editing: &'a mut BoardEditing,
    pub(crate) canvas: &'a mut crate::canvas::CanvasState,
    pub(crate) library: Option<&'a Library>,
    pub(crate) active_board_id: &'a mut i64,
    pub(crate) search: &'a mut crate::search::SearchController,
    pub(crate) placement_creation: &'a mut crate::placement::content_resolution::PlacementCreation,
    pub(crate) residency: &'a mut crate::canvas::residency::ImageResidencyCoordinator,
    pub(crate) worker: Option<&'a crate::image_preparation::ImagePrepWorker>,
    pub(crate) max_texture_side: usize,
    pub(crate) session: UserSession,
    pub(crate) trace_enabled: bool,
}

struct HistoryCanvasReplay {
    document_changes: crate::canvas::CanvasDocumentChanges,
    placements_needing_content: Vec<crate::data::Placement>,
}

impl BoardLifecycle<'_> {
    pub(crate) fn undo_document_history(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<String>, String> {
        self.replay_document_history(ctx, HistoryDirection::Backward)
    }

    pub(crate) fn redo_document_history(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<Option<String>, String> {
        self.replay_document_history(ctx, HistoryDirection::Forward)
    }

    fn replay_document_history(
        &mut self,
        ctx: &egui::Context,
        direction: HistoryDirection,
    ) -> Result<Option<String>, String> {
        if self.library.is_none() {
            return Err("Storage unavailable".to_owned());
        }
        let owner_before = *self.active_board_id;
        let applied = match direction {
            HistoryDirection::Backward => self.editing.history.undo(owner_before),
            HistoryDirection::Forward => self.editing.history.redo(owner_before),
        };
        let Some(applied) = applied else {
            return Ok(None);
        };
        let owner_after = applied
            .action
            .owner_after_replay(owner_before, applied.direction);
        self.editing
            .history
            .reassign_applied(owner_before, owner_after, applied.direction);
        self.apply_history_action(ctx, &applied.action, applied.direction)?;
        let verb = match direction {
            HistoryDirection::Backward => "Undid",
            HistoryDirection::Forward => "Redid",
        };
        Ok(Some(format!("{verb} {}", applied.action.label())))
    }

    pub(crate) fn apply_history_action(
        &mut self,
        ctx: &egui::Context,
        action: &HistoryAction,
        direction: HistoryDirection,
    ) -> Result<(), String> {
        match action {
            HistoryAction::Document(document_action) => {
                let replay = self.apply_history_to_active_canvas(document_action, direction);
                self.editing
                    .enqueue_document_changes(*self.active_board_id, replay.document_changes)?;
                self.load_history_placement_content(ctx, replay.placements_needing_content)
            }
            HistoryAction::Board(lifecycle_action) => {
                self.apply_board_lifecycle_history(ctx, lifecycle_action, direction)
            }
        }
    }

    pub(crate) fn accepted_board_document(
        &self,
        library: &Library,
        board_id: i64,
    ) -> Result<BoardDocumentSnapshot, String> {
        self.editing.accepted_board_document(library, board_id)
    }

    fn apply_board_lifecycle_history(
        &mut self,
        ctx: &egui::Context,
        action: &BoardLifecycleHistoryAction,
        direction: HistoryDirection,
    ) -> Result<(), String> {
        let Some(library) = self.library else {
            return Err("Storage unavailable".to_owned());
        };
        if let BoardLifecycleHistoryAction::Renamed { board_id, .. } = action {
            self.editing.documents.ensure_loaded(library, *board_id)?;
        }
        let active_before_replay = *self.active_board_id;
        match (action, direction) {
            (
                BoardLifecycleHistoryAction::Created {
                    document,
                    active_before,
                },
                HistoryDirection::Backward,
            ) => {
                let active_document = self.accepted_board_document(&library, *active_before)?;
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                        board_id: document.board.id,
                        active_board_id: *active_before,
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.archive(document.board.id);
                self.open_board_document_snapshot(Some(ctx), &active_document)?;
            }
            (BoardLifecycleHistoryAction::Created { document, .. }, HistoryDirection::Forward) => {
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Restore {
                        document: document.clone(),
                        active_board_id: document.board.id,
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.restore_document(document);
                self.open_board_document_snapshot(Some(ctx), document)?;
            }
            (
                BoardLifecycleHistoryAction::Renamed {
                    board_id,
                    before_name,
                    ..
                },
                HistoryDirection::Backward,
            ) => {
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                        board_id: *board_id,
                        name: before_name.clone(),
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.rename(*board_id, before_name);
            }
            (
                BoardLifecycleHistoryAction::Renamed {
                    board_id,
                    after_name,
                    ..
                },
                HistoryDirection::Forward,
            ) => {
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Rename {
                        board_id: *board_id,
                        name: after_name.clone(),
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.rename(*board_id, after_name);
            }
            (
                BoardLifecycleHistoryAction::Deleted {
                    document,
                    active_before,
                    ..
                },
                HistoryDirection::Backward,
            ) => {
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Restore {
                        document: document.clone(),
                        active_board_id: *active_before,
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.restore_document(document);
                if active_before_replay != *active_before {
                    self.open_board_document_snapshot(Some(ctx), document)?;
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
                self.editing.persistence.enqueue_board_lifecycle_unit(
                    crate::active_board::persistence::BoardLifecyclePersistence::Archive {
                        board_id: document.board.id,
                        active_board_id: *active_after,
                    },
                    &self.editing.history,
                )?;
                self.editing.documents.archive(document.board.id);
                if let Some(active_document) = active_document {
                    self.open_board_document_snapshot(Some(ctx), &active_document)?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn open_board_document_snapshot(
        &mut self,
        ctx: Option<&egui::Context>,
        document: &crate::data::BoardDocumentSnapshot,
    ) -> Result<(), String> {
        self.search.close_overlay();
        self.search.reset_embedding_backfill();
        self.canvas.clear_for_board_switch();
        *self.active_board_id = document.board.id;
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
        self.queue_opened_board_images(ctx, document.placements.clone())?;
        self.save_user_session()?;
        Ok(())
    }

    fn apply_history_to_active_canvas(
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
            self.placement_creation
                .invalidate_placement(target.state.id);
            if let Some(placement) = self
                .canvas
                .apply_placement_history_state(Some(target.clone()))
            {
                placements_needing_content.push(placement);
            }
            document_changes.updated_placements.push(target);
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

    fn load_history_placement_content(
        &mut self,
        ctx: &egui::Context,
        placements: Vec<crate::data::Placement>,
    ) -> Result<(), String> {
        let Some(library) = self.library else {
            return Err("Storage unavailable".to_owned());
        };
        for placement in placements {
            if let Some(import_worker) = &self.worker {
                self.placement_creation.enqueue_history_placement(
                    import_worker,
                    *self.active_board_id,
                    placement,
                    self.max_texture_side,
                    crate::image_preparation::CanvasImageSettings {
                        max_side: self.session.settings.canvas_image_max_side,
                    },
                )?;
                continue;
            }
            match crate::image_preparation::load_existing(
                &library,
                *self.active_board_id,
                None,
                placement,
                self.max_texture_side,
                crate::image_preparation::CanvasImageSettings {
                    max_side: self.session.settings.canvas_image_max_side,
                },
                crate::data::CanvasImageLod::Working,
                None,
                None,
            ) {
                crate::image_preparation::ImagePrepResult::Loaded(loaded) => {
                    self.canvas.add_prepared_placement_image(ctx, loaded);
                }
                crate::image_preparation::ImagePrepResult::Failed { error, .. } => {
                    return Err(error);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn save_user_session(&mut self) -> Result<(), String> {
        self.session.active_board_id = *self.active_board_id;
        self.session.viewport = self.canvas.viewport_state();
        self.editing
            .persistence
            .save_user_session(self.session.clone())
    }

    fn queue_current(&mut self) -> Result<(), String> {
        self.editing.record_pending_document_changes_history(
            self.canvas,
            self.library,
            *self.active_board_id,
            "Edit board",
        )?;
        self.save_user_session()
    }

    fn flush_current(&mut self) -> Result<(), String> {
        self.queue_current()?;
        self.editing.persistence.flush()
    }

    fn queue_opened_board_images(
        &mut self,
        ctx: Option<&egui::Context>,
        placements: Vec<crate::data::Placement>,
    ) -> Result<(), String> {
        if let Some(worker) = self.worker {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 640.0));
            let viewport = egui::Rect::from_min_max(
                self.canvas.screen_to_world(screen.min, screen),
                self.canvas.screen_to_world(screen.max, screen),
            );
            self.residency.queue_startup_existing_loads(
                worker,
                self.canvas,
                placements,
                viewport,
                *self.active_board_id,
                &self.session.settings,
                self.max_texture_side,
                self.trace_enabled,
            )
        } else if let Some(ctx) = ctx {
            self.load_history_placement_content(ctx, placements)
        } else {
            Ok(())
        }
    }

    pub(crate) fn activate(&mut self, board_id: i64) -> Result<(), String> {
        if board_id == *self.active_board_id {
            return Ok(());
        }
        let library = self.library.ok_or("Storage unavailable")?;
        // Resolve before draining or clearing the current Board. Storage
        // activation may fail, in which case its Canvas remains usable.
        let document = self.accepted_board_document(library, board_id)?;
        self.flush_current()?;
        library.set_active_board(board_id)?;
        self.dedupe_for_activation()?;
        let document = self
            .accepted_board_document(library, board_id)
            .unwrap_or(document);
        self.open_board_document_snapshot(None, &document)
    }

    pub(crate) fn reload_active(&mut self) -> Result<(), String> {
        self.dedupe_for_activation()?;
        self.restore()
    }

    fn dedupe_for_activation(&mut self) -> Result<(), String> {
        self.editing.persistence.flush()?;
        if let Some(library) = self.library {
            let report = library.dedupe_exact_assets(32, 8)?;
            if report.changed() {
                self.editing
                    .remap_assets(library, &report.asset_id_remaps)?;
                self.search.invalidate_library_search_cache();
            }
        }
        Ok(())
    }

    pub(crate) fn restore(&mut self) -> Result<(), String> {
        let library = self.library.ok_or("Storage unavailable")?;
        let document = self.accepted_board_document(library, *self.active_board_id)?;
        self.open_board_document_snapshot(None, &document)
    }

    pub(crate) fn create(&mut self) -> Result<String, String> {
        let library = self.library.ok_or("Storage unavailable")?;
        let board = library.new_board(generated_board_name(self.editing.documents.summaries()));
        let document = BoardDocumentSnapshot {
            board: board.clone(),
            placements: Vec::new(),
            annotations: Vec::new(),
            groups: Vec::new(),
            stack_order: Vec::new(),
            viewport: Default::default(),
        };
        self.queue_current()?;
        self.editing.history.record_applied(
            board.id,
            HistoryAction::Board(BoardLifecycleHistoryAction::Created {
                document: document.clone(),
                active_before: *self.active_board_id,
            }),
        );
        self.editing.persistence.enqueue_board_lifecycle_unit(
            super::persistence::BoardLifecyclePersistence::Create {
                document: document.clone(),
                active_board_id: board.id,
            },
            &self.editing.history,
        )?;
        self.editing.documents.restore_document(&document);
        self.open_board_document_snapshot(None, &document)?;
        Ok(format!("Created {}", board.name))
    }

    pub(crate) fn rename(&mut self, board_id: i64, name: &str) -> Result<Option<String>, String> {
        let library = self.library.ok_or("Storage unavailable")?;
        self.editing.accepted_board_document(library, board_id)?;
        let before = self
            .editing
            .documents
            .summaries()
            .into_iter()
            .find(|summary| summary.board.id == board_id)
            .map(|summary| summary.board.name.clone())
            .ok_or("Board unavailable")?;
        let after = name.trim().to_owned();
        if after.is_empty() || after == before {
            return Ok(None);
        }
        self.editing.history.record_applied(
            *self.active_board_id,
            HistoryAction::Board(BoardLifecycleHistoryAction::Renamed {
                board_id,
                before_name: before,
                after_name: after.clone(),
            }),
        );
        self.editing.documents.rename(board_id, &after);
        self.editing.persistence.enqueue_board_lifecycle_unit(
            super::persistence::BoardLifecyclePersistence::Rename {
                board_id,
                name: after,
            },
            &self.editing.history,
        )?;
        Ok(Some("Renamed board".to_owned()))
    }

    pub(crate) fn delete(&mut self, ctx: &egui::Context, board_id: i64) -> Result<String, String> {
        let library = self.library.ok_or("Storage unavailable")?;
        let board_ids = self
            .editing
            .documents
            .summaries()
            .into_iter()
            .map(|summary| summary.board.id)
            .collect::<Vec<_>>();
        let index = board_ids
            .iter()
            .position(|id| *id == board_id)
            .ok_or("Board unavailable")?;
        if board_ids.len() == 1 {
            return Err("cannot delete the last board".to_owned());
        }
        let active_before = *self.active_board_id;
        let active_after = if active_before != board_id {
            active_before
        } else if index > 0 {
            board_ids[index - 1]
        } else {
            board_ids[index + 1]
        };
        let active_document = if active_before != active_after {
            Some(self.accepted_board_document(library, active_after)?)
        } else {
            None
        };
        self.queue_current()?;
        let mut document = self.accepted_board_document(library, board_id)?;
        if board_id == active_before {
            document.viewport = self.canvas.viewport_state();
        }
        let name = document.board.name.clone();
        self.editing.history.record_applied(
            active_after,
            HistoryAction::Board(BoardLifecycleHistoryAction::Deleted {
                document,
                active_before,
                active_after,
            }),
        );
        self.editing.persistence.enqueue_board_lifecycle_unit(
            super::persistence::BoardLifecyclePersistence::Archive {
                board_id,
                active_board_id: active_after,
            },
            &self.editing.history,
        )?;
        self.editing.documents.archive(board_id);
        if let Some(document) = active_document {
            self.open_board_document_snapshot(Some(ctx), &document)?;
        }
        Ok(format!("Deleted {name}"))
    }
}

fn generated_board_name(boards: &[crate::data::BoardSummary]) -> String {
    let names = boards
        .iter()
        .map(|summary| summary.board.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    (2..)
        .map(|number| format!("Board {number}"))
        .find(|name| !names.contains(name.as_str()))
        .unwrap_or_else(|| "Board 2".to_owned())
}
