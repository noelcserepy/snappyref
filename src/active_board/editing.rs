//! Accepts Board edits and owns their history and ordered durability.
//! Canvas gestures stay in memory until their caller submits one completed edit.
use crate::active_board::history::{DocumentHistory, HistoryAction, HistoryChange};
use crate::active_board::persistence::DocumentPersistence;

#[derive(Default)]
pub(crate) struct BoardEditing {
    pub(crate) history: DocumentHistory,
    pub(crate) documents: super::catalog::AcceptedBoards,
    pub(crate) persistence: DocumentPersistence,
}

impl BoardEditing {
    pub(crate) fn record_pending_document_changes_history(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::library::storage::Library>,
        board_id: i64,
        label: &str,
    ) -> Result<bool, String> {
        self.record_pending_changes(canvas, library, board_id, label, false)
    }

    /// Edits addressed by id, deletions included, as one Undo step.
    pub(crate) fn accept_direct_edits(
        &mut self,
        context: &mut BoardEditContext<'_>,
        label: &str,
        reload: Vec<crate::data::Placement>,
    ) -> Result<bool, String> {
        let changed = self.record_pending_changes(
            context.canvas,
            context.library,
            context.board_id,
            label,
            true,
        )?;
        context.reload(reload)?;
        Ok(changed)
    }

    /// Gesture deletions record their own history, so only direct edits ask
    /// for `deletions` here.
    fn record_pending_changes(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::library::storage::Library>,
        board_id: i64,
        label: &str,
        deletions: bool,
    ) -> Result<bool, String> {
        self.documents
            .set_viewport(board_id, canvas.viewport_state());
        canvas.reflow_dirty_group_members();
        let Some(library) = library else {
            return Ok(false);
        };
        let changes = canvas.pending_document_changes_preview();
        if !changes.has_changes() {
            return Ok(false);
        }

        let current_document = self.accepted_board_document(library, board_id).ok();
        let current_items = current_document
            .as_ref()
            .map(|document| document.placements.as_slice())
            .unwrap_or_default();
        let current_annotations = current_document
            .as_ref()
            .map(|document| document.annotations.as_slice())
            .unwrap_or_default();
        let current_stack_order = current_document
            .as_ref()
            .map(|document| document.stack_order.clone());
        let mut placement_changes = changes
            .updated_placements
            .into_iter()
            .map(|after| {
                let before = current_items
                    .iter()
                    .find(|placement| placement.state.id == after.state.id)
                    .cloned();
                HistoryChange {
                    before,
                    after: Some(after),
                }
            })
            .collect::<Vec<_>>();
        let mut annotation_changes = changes
            .updated_annotations
            .into_iter()
            .map(|after| {
                let before = current_annotations
                    .iter()
                    .find(|annotation| annotation.id == after.id)
                    .cloned();
                HistoryChange {
                    before,
                    after: Some(after),
                }
            })
            .collect::<Vec<_>>();
        if deletions {
            placement_changes.extend(
                current_items
                    .iter()
                    .filter(|placement| {
                        changes.archived_placement_ids.contains(&placement.state.id)
                    })
                    .map(|placement| {
                        let mut archived = placement.clone();
                        archived.state.archived = true;
                        HistoryChange {
                            before: Some(placement.clone()),
                            after: Some(archived),
                        }
                    }),
            );
            annotation_changes.extend(
                current_annotations
                    .iter()
                    .filter(|annotation| changes.archived_annotation_ids.contains(&annotation.id))
                    .map(|annotation| {
                        let mut archived = annotation.clone();
                        archived.archived = true;
                        HistoryChange {
                            before: Some(annotation.clone()),
                            after: Some(archived),
                        }
                    }),
            );
        }
        let stack_order = changes.stack_order.map(|after| HistoryChange {
            before: current_stack_order,
            after: Some(after),
        });

        let groups = changes.groups.map(|after| HistoryChange {
            before: current_document
                .as_ref()
                .map(|document| document.groups.clone()),
            after: Some(after),
        });
        self.history.record_applied(
            board_id,
            HistoryAction::document(
                label.to_owned(),
                placement_changes,
                annotation_changes,
                stack_order,
            )
            .with_groups(groups),
        );
        self.queue_canvas_document(canvas, board_id)
    }
}

pub(crate) struct BoardEditContext<'a> {
    pub(crate) canvas: &'a mut crate::canvas::CanvasState,
    pub(crate) library: Option<&'a crate::library::storage::Library>,
    pub(crate) board_id: i64,
    pub(crate) worker: Option<&'a crate::image_preparation::ImagePrepWorker>,
    pub(crate) resolution: &'a mut crate::placement::content_resolution::PlacementCreation,
    pub(crate) max_texture_side: usize,
    pub(crate) settings: &'a crate::data::WorkspaceSettings,
    pub(crate) viewport_world: eframe::egui::Rect,
}
impl BoardEditContext<'_> {
    fn command(&mut self) -> super::commands::ActiveBoardCommandContext<'_> {
        super::commands::ActiveBoardCommandContext {
            canvas: self.canvas,
            library: self.library,
            import_worker_available: self.worker.is_some(),
            placement_gap: self.settings.placement_gap,
            viewport_world: self.viewport_world,
        }
    }
    fn reload(&mut self, placements: Vec<crate::data::Placement>) -> Result<(), String> {
        let Some(worker) = self.worker else {
            return Ok(());
        };
        for placement in placements {
            self.resolution.enqueue_history_placement(
                worker,
                self.board_id,
                placement,
                self.max_texture_side,
                crate::image_preparation::CanvasImageSettings {
                    max_side: self.settings.canvas_image_max_side,
                },
            )?;
        }
        Ok(())
    }
}
impl BoardEditing {
    pub(crate) fn paste_payload(
        &mut self,
        context: &mut BoardEditContext<'_>,
        payload: &crate::sources::clipboard::ClipboardBoardSelection,
        anchor: eframe::egui::Pos2,
    ) -> Result<String, String> {
        let outcome =
            super::commands::paste_board_payload_at(&mut context.command(), payload, anchor)?;
        self.accept_paste(context, outcome)
    }
    pub(crate) fn duplicate(
        &mut self,
        context: &mut BoardEditContext<'_>,
    ) -> Result<Option<String>, String> {
        let Some(outcome) = super::commands::duplicate_selection(&mut context.command())? else {
            return Ok(None);
        };
        self.accept_paste(context, outcome).map(Some)
    }
    fn accept_paste(
        &mut self,
        context: &mut BoardEditContext<'_>,
        outcome: super::commands::PasteCommandOutcome,
    ) -> Result<String, String> {
        self.record_pending_document_changes_history(
            context.canvas,
            context.library,
            context.board_id,
            &outcome.status,
        )?;
        context.reload(outcome.reload)?;
        Ok(outcome.status)
    }
    pub(crate) fn restack_selection(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        board_id: i64,
        front: bool,
    ) -> Result<(), String> {
        let before = canvas.canvas_stack_order();
        let changed = if front {
            canvas.bring_selection_to_front()
        } else {
            canvas.send_selection_to_back()
        };
        if changed {
            self.history.record_applied(
                board_id,
                HistoryAction::document(
                    if front {
                        "Bring selection to front"
                    } else {
                        "Send selection to back"
                    }
                    .to_owned(),
                    Vec::new(),
                    Vec::new(),
                    Some(HistoryChange {
                        before: Some(before),
                        after: Some(canvas.canvas_stack_order()),
                    }),
                ),
            );
            self.queue_canvas_document(canvas, board_id)?;
        }
        Ok(())
    }
}

impl BoardEditing {
    pub(crate) fn accepted_placement(
        &self,
        board_id: i64,
        placement_id: i64,
    ) -> Option<crate::data::Placement> {
        self.documents.placement(board_id, placement_id)
    }

    pub(crate) fn accepted_board_document(
        &self,
        library: &crate::library::storage::Library,
        board_id: i64,
    ) -> Result<crate::data::BoardDocumentSnapshot, String> {
        self.documents.document(library, board_id)
    }

    /// Completion replaces accepted content in the existing edit, without
    /// adding an Undo entry. The queued write carries the complete Placement.
    pub(crate) fn resolve_placements(
        &mut self,
        board_id: i64,
        placements: Vec<crate::data::Placement>,
    ) -> Result<bool, String> {
        self.resolve_document_changes(
            board_id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: placements,
                ..Default::default()
            },
        )
    }

    /// Arrival can reflow Group members that the arriving Placement's edit never touched.
    /// That edit adopts them, so Undo and Redo move them with the arrival.
    pub(crate) fn resolve_group_arrival(
        &mut self,
        board_id: i64,
        arrival: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<bool, String> {
        self.history.adopt_completion_changes(
            board_id,
            arrival,
            changes
                .updated_placements
                .iter()
                .filter_map(|p| self.documents.placement(board_id, p.state.id))
                .collect(),
            changes
                .updated_annotations
                .iter()
                .filter_map(|a| self.documents.annotation(board_id, a.id))
                .collect(),
        );
        self.resolve_document_changes(board_id, changes)
    }

    /// Batch completion can also finish text editing or reflow Groups. Submit
    /// every resulting document change without adding a second batch Undo entry.
    pub(crate) fn resolve_document_changes(
        &mut self,
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<bool, String> {
        let changed = self
            .history
            .update_accepted_document_changes(board_id, &changes);
        self.enqueue_document_changes(board_id, changes)?;
        Ok(changed)
    }

    pub(crate) fn fail_placement(
        &mut self,
        board_id: i64,
        placement_id: i64,
        message: String,
    ) -> Result<Option<crate::data::Placement>, String> {
        let Some(mut placement) = self.documents.placement(board_id, placement_id) else {
            return Ok(None);
        };
        let source = match &placement.content {
            crate::data::PlacementContent::Loading { source, .. } => Some(source.clone()),
            crate::data::PlacementContent::Failed { source, .. } => source.clone(),
            crate::data::PlacementContent::Loaded { .. } => None,
        };
        placement.content = crate::data::PlacementContent::Failed {
            source,
            message,
            recoverable: true,
        };
        self.history
            .update_accepted_placement(board_id, placement.clone());
        self.enqueue_document_changes(
            board_id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![placement.clone()],
                ..Default::default()
            },
        )?;
        Ok(Some(placement))
    }

    pub(crate) fn accept_crop(
        &mut self,
        ctx: &eframe::egui::Context,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::library::storage::Library>,
        board_id: i64,
        cropped: crate::image_preparation::PreparedCropPreview,
    ) -> Result<super::crop::CroppedImageIntegration, String> {
        let integration = super::crop::integrate_cropped_result(ctx, canvas, library, cropped)?;
        self.history.record_applied(
            board_id,
            HistoryAction::document(
                "Crop image".to_owned(),
                vec![HistoryChange {
                    before: Some(integration.previous_record.clone()),
                    after: Some(integration.updated_record.clone()),
                }],
                Vec::new(),
                None,
            ),
        );
        self.queue_canvas_document(canvas, board_id)?;
        Ok(integration)
    }
}

impl BoardEditing {
    pub(crate) fn accept_created_batch(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        board_id: i64,
        action: Option<HistoryAction>,
    ) -> Result<(), String> {
        if let Some(action) = action {
            self.history.record_applied(board_id, action);
        }
        self.queue_canvas_document(canvas, board_id)?;
        Ok(())
    }
}

impl BoardEditing {
    pub(crate) fn delete_selection(
        &mut self,
        context: &mut BoardEditContext<'_>,
    ) -> Result<Option<String>, String> {
        let before_groups = context.canvas.groups().to_vec();
        let before_all = context.canvas.committed_placements();
        let before_all_annotations = context.canvas.annotations();
        let before_items = context
            .canvas
            .selected_placement_ids()
            .into_iter()
            .filter_map(|placement_id| context.canvas.placement_for_id(placement_id))
            .collect::<Vec<_>>();
        let before_annotations = context
            .canvas
            .selected_annotation_ids()
            .into_iter()
            .filter_map(|annotation_id| context.canvas.annotation_for_id(annotation_id))
            .collect::<Vec<_>>();
        let before_stack_order = context.canvas.canvas_stack_order();
        match crate::active_board::commands::delete_selected_from_board(&mut context.command()) {
            Ok(Some(status)) => {
                let mut placement_changes: Vec<_> = before_items
                    .into_iter()
                    .map(|placement| {
                        let mut archived = placement.clone();
                        archived.state.archived = true;
                        HistoryChange {
                            before: Some(placement),
                            after: Some(archived),
                        }
                    })
                    .collect();
                placement_changes.extend(
                    context
                        .canvas
                        .pending_document_changes_preview()
                        .updated_placements
                        .iter()
                        .filter_map(|state| {
                            let after = context.canvas.placement_for_id(state.state.id)?;
                            let before = before_all
                                .iter()
                                .find(|p| p.state.id == state.state.id)?
                                .clone();
                            Some(HistoryChange {
                                before: Some(before),
                                after: Some(after),
                            })
                        }),
                );
                let mut annotation_changes: Vec<_> = before_annotations
                    .into_iter()
                    .map(|annotation| {
                        let mut archived = annotation.clone();
                        archived.archived = true;
                        HistoryChange {
                            before: Some(annotation),
                            after: Some(archived),
                        }
                    })
                    .collect();
                annotation_changes.extend(
                    context
                        .canvas
                        .pending_document_changes_preview()
                        .updated_annotations
                        .into_iter()
                        .filter_map(|after| {
                            let before = before_all_annotations
                                .iter()
                                .find(|a| a.id == after.id)?
                                .clone();
                            Some(HistoryChange {
                                before: Some(before),
                                after: Some(after),
                            })
                        }),
                );
                self.history.record_applied(
                    context.board_id,
                    HistoryAction::document(
                        status.clone(),
                        placement_changes,
                        annotation_changes,
                        Some(HistoryChange {
                            before: Some(before_stack_order),
                            after: Some(context.canvas.canvas_stack_order()),
                        }),
                    )
                    .with_groups(Some(HistoryChange {
                        before: Some(before_groups),
                        after: Some(context.canvas.groups().to_vec()),
                    })),
                );
                self.queue_canvas_document(context.canvas, context.board_id)?;
                Ok(Some(status))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub(crate) fn accept_pending_annotations(
        &mut self,
        context: &mut BoardEditContext<'_>,
    ) -> Result<Option<String>, String> {
        let mut status = None;
        let Some(library) = context.library else {
            return Ok(None);
        };

        while let Some(request) = context.canvas.take_pending_annotation() {
            let before_stack_order = context.canvas.canvas_stack_order();
            let style = context.canvas.next_annotation_style();
            let mut annotation = library.new_canvas_annotation(
                request.kind,
                request.start_world(),
                request.end_world(),
                String::new(),
                style.text_size,
                style.stroke_width,
                style.color,
            );
            annotation.fill = style.fill;
            let should_focus_text = annotation.kind == crate::data::CanvasAnnotationKind::Textbox;
            let annotation_id = annotation.id;
            context.canvas.add_annotation(annotation);
            context
                .canvas
                .mark_annotation_for_persistence(annotation_id);
            if let Some(created) = context.canvas.annotation_for_id(annotation_id) {
                self.history.record_applied(
                    context.board_id,
                    HistoryAction::document(
                        match request.kind {
                            crate::data::CanvasAnnotationKind::Arrow => "Added arrow",
                            crate::data::CanvasAnnotationKind::Line => "Added line",
                            crate::data::CanvasAnnotationKind::Textbox => "Added textbox",
                            crate::data::CanvasAnnotationKind::Rectangle => "Added rectangle",
                        }
                        .to_owned(),
                        Vec::new(),
                        vec![HistoryChange {
                            before: None,
                            after: Some(created),
                        }],
                        Some(HistoryChange {
                            before: Some(before_stack_order),
                            after: Some(context.canvas.canvas_stack_order()),
                        }),
                    ),
                );
                self.queue_canvas_document(context.canvas, context.board_id)?;
            }
            // A new annotation is selected so the style keys reach it at once.
            if should_focus_text {
                context.canvas.begin_textbox_edit(annotation_id);
            } else {
                context.canvas.select_content(&[], &[annotation_id]);
            }
            status = Some(match request.kind {
                crate::data::CanvasAnnotationKind::Arrow => "Added arrow".to_owned(),
                crate::data::CanvasAnnotationKind::Line => "Added line".to_owned(),
                crate::data::CanvasAnnotationKind::Textbox => "Added textbox".to_owned(),
                crate::data::CanvasAnnotationKind::Rectangle => "Added rectangle".to_owned(),
            });
        }
        Ok(status)
    }
}

impl BoardEditing {
    pub(crate) fn group_selection(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::library::storage::Library>,
        board_id: i64,
    ) -> Result<(), String> {
        let library = library.ok_or("Storage unavailable")?;
        canvas.create_group(library.allocate_document_id())?;
        self.record_pending_document_changes_history(
            canvas,
            Some(library),
            board_id,
            "Edit board",
        )?;
        Ok(())
    }

    pub(crate) fn ungroup_selection(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        library: Option<&crate::library::storage::Library>,
        board_id: i64,
    ) -> Result<(), String> {
        canvas.ungroup_selected();
        self.record_pending_document_changes_history(canvas, library, board_id, "Edit board")?;
        Ok(())
    }
}

impl BoardEditing {
    fn queue_canvas_document(
        &mut self,
        canvas: &mut crate::canvas::CanvasState,
        board_id: i64,
    ) -> Result<bool, String> {
        if self.persistence.is_available() {
            self.documents
                .apply_changes(board_id, &canvas.pending_document_changes_preview());
        }
        self.persistence
            .queue_canvas_document(canvas, board_id, &self.history)
    }

    pub(crate) fn enqueue_document_changes(
        &mut self,
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<(), String> {
        if self.persistence.is_available() {
            self.documents.apply_changes(board_id, &changes);
        }
        self.persistence
            .enqueue_document_unit(board_id, changes, &self.history)
    }
}

impl BoardEditing {
    pub(crate) fn remap_assets(
        &mut self,
        library: &crate::library::storage::Library,
        remaps: &[(i64, i64)],
    ) -> Result<(), String> {
        let canonical =
            library.load_assets(&remaps.iter().map(|(_, id)| *id).collect::<Vec<_>>())?;
        let assets = remaps
            .iter()
            .filter_map(|(old, new)| canonical.get(new).map(|asset| (*old, asset.clone())))
            .collect();
        self.documents.remap_assets(&assets);
        self.history.remap_assets(&assets);
        Ok(())
    }
}

pub(super) fn remap_placement_asset(
    placement: &mut crate::data::Placement,
    assets: &std::collections::HashMap<i64, crate::data::Asset>,
) {
    use crate::data::{ContentResolutionSource, PlacementContent};
    match &mut placement.content {
        PlacementContent::Loaded { asset } => {
            if let Some(canonical) = assets.get(&asset.id) {
                *asset = canonical.clone();
            }
        }
        PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id },
            ..
        }
        | PlacementContent::Failed {
            source: Some(ContentResolutionSource::SearchAsset { asset_id }),
            ..
        } => {
            if let Some(canonical) = assets.get(asset_id) {
                *asset_id = canonical.id;
            }
        }
        _ => {}
    }
}
