use super::*;

pub(crate) enum BoardDurabilityOperation {
    Document {
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    },
    Lifecycle(crate::active_board::persistence::BoardLifecyclePersistence),
    #[cfg(test)]
    HistoryOnly,
}

impl Library {
    pub(crate) fn persist_board_unit(
        &self,
        operation: &BoardDurabilityOperation,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<(), String> {
        let history_payload = history.encode()?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to start Board durability transaction: {error}"))?;

        match operation {
            BoardDurabilityOperation::Document { board_id, changes } => {
                persist_document_changes(&transaction, *board_id, changes, history)?;
            }
            BoardDurabilityOperation::Lifecycle(change) => {
                persist_lifecycle_change(&transaction, change)?;
            }
            #[cfg(test)]
            BoardDurabilityOperation::HistoryOnly => {}
        }
        transaction
            .execute(
                "INSERT INTO board_document_history (id, payload)
                 VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
                params![history_payload],
            )
            .map_err(|error| format!("failed to persist Board history: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit Board durability transaction: {error}"))
    }

    pub(crate) fn load_document_history(
        &self,
    ) -> Result<crate::active_board::history::DocumentHistory, String> {
        let connection = self.connection()?;
        let payload = match connection.query_row(
            "SELECT payload FROM board_document_history WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(payload) => payload,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(Default::default()),
            Err(error) => return Err(format!("failed to load Board history: {error}")),
        };
        crate::active_board::history::DocumentHistory::decode(&payload, self)
    }
}

fn persist_document_changes(
    transaction: &Transaction<'_>,
    board_id: i64,
    changes: &crate::canvas::CanvasDocumentChanges,
    history: &crate::active_board::history::DocumentHistory,
) -> Result<(), String> {
    for placement_id in &changes.archived_placement_ids {
        transaction
            .execute(
                "UPDATE board_items SET archived = 1 WHERE id = ?1 AND board_id = ?2",
                params![placement_id, board_id],
            )
            .map_err(|error| format!("failed to archive placement {placement_id}: {error}"))?;
    }
    for annotation_id in &changes.archived_annotation_ids {
        transaction
            .execute(
                "UPDATE canvas_annotations SET archived = 1 WHERE id = ?1 AND board_id = ?2",
                params![annotation_id, board_id],
            )
            .map_err(|error| {
                format!("failed to archive canvas annotation {annotation_id}: {error}")
            })?;
    }
    for item in &changes.updated_placements {
        let mut placement = history
            .accepted_placement(board_id, item.id)
            .ok_or_else(|| format!("missing accepted placement {}", item.id))?;
        placement.state = item.clone();
        super::placements::restore_board_placement(transaction, board_id, &placement)?;
    }
    for annotation in &changes.updated_annotations {
        let changed = transaction
            .execute(
                "UPDATE canvas_annotations
                 SET start_x = ?1, start_y = ?2, end_x = ?3, end_y = ?4,
                     text = ?5, text_size = ?6, stroke_width = ?7, color = ?8,
                     background_enabled = ?9, archived = ?10
                 WHERE id = ?11 AND board_id = ?12",
                params![
                    annotation.start().x,
                    annotation.start().y,
                    annotation.end().x,
                    annotation.end().y,
                    annotation.text,
                    annotation.text_size.as_str(),
                    annotation.stroke_width.as_str(),
                    annotation.color.as_str(),
                    if annotation.background_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    if annotation.archived { 1_i64 } else { 0_i64 },
                    annotation.id,
                    board_id,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to update canvas annotation {}: {error}",
                    annotation.id
                )
            })?;
        if changed == 0 {
            transaction
                .execute(
                    "INSERT INTO canvas_annotations (
                        id, board_id, kind, start_x, start_y, end_x, end_y, text,
                        text_size, stroke_width, color, background_enabled, archived
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        annotation.id,
                        board_id,
                        annotation.kind.as_str(),
                        annotation.start().x,
                        annotation.start().y,
                        annotation.end().x,
                        annotation.end().y,
                        annotation.text,
                        annotation.text_size.as_str(),
                        annotation.stroke_width.as_str(),
                        annotation.color.as_str(),
                        if annotation.background_enabled {
                            1_i64
                        } else {
                            0_i64
                        },
                        if annotation.archived { 1_i64 } else { 0_i64 },
                    ],
                )
                .map_err(|error| {
                    format!(
                        "failed to insert canvas annotation {}: {error}",
                        annotation.id
                    )
                })?;
        }
    }
    if let Some(stack_order) = &changes.stack_order {
        transaction
            .execute(
                "DELETE FROM canvas_stack_items WHERE board_id = ?1",
                params![board_id],
            )
            .map_err(|error| format!("failed to clear canvas stack order: {error}"))?;
        for (stack_index, item) in stack_order.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO canvas_stack_items (board_id, item_kind, item_id, stack_index)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![board_id, item.kind(), item.item_id(), stack_index as i64],
                )
                .map_err(|error| format!("failed to save canvas stack order: {error}"))?;
        }
    }
    Ok(())
}

fn persist_lifecycle_change(
    transaction: &Transaction<'_>,
    change: &crate::active_board::persistence::BoardLifecyclePersistence,
) -> Result<(), String> {
    use crate::active_board::persistence::BoardLifecyclePersistence;
    match change {
        BoardLifecyclePersistence::Create {
            document,
            active_board_id,
        } => {
            transaction
                .execute(
                    "INSERT INTO boards (
                        id, name, archived, camera_pan_x, camera_pan_y, camera_zoom,
                        canvas_background
                     ) VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name,
                        archived = 0,
                        camera_pan_x = excluded.camera_pan_x,
                        camera_pan_y = excluded.camera_pan_y,
                        camera_zoom = excluded.camera_zoom,
                        canvas_background = excluded.canvas_background",
                    params![
                        document.board.id,
                        document.board.name,
                        document.viewport.camera_pan_x,
                        document.viewport.camera_pan_y,
                        document.viewport.camera_zoom,
                        document.viewport.canvas_background.as_str(),
                    ],
                )
                .map_err(|error| {
                    format!("failed to create board {}: {error}", document.board.id)
                })?;
            super::boards::persist_active_board_id(transaction, *active_board_id)?;
        }
        BoardLifecyclePersistence::Rename { board_id, name } => {
            let trimmed_name = name.trim();
            if !trimmed_name.is_empty() {
                transaction
                    .execute(
                        "UPDATE boards SET name = ?1 WHERE id = ?2 AND archived = 0",
                        params![trimmed_name, board_id],
                    )
                    .map_err(|error| format!("failed to rename board {board_id}: {error}"))?;
            }
        }
        BoardLifecyclePersistence::Archive {
            board_id,
            active_board_id,
        } => {
            if board_id == active_board_id {
                return Err(format!("cannot keep archived board {board_id} active"));
            }
            if !super::boards::board_is_non_archived(transaction, *active_board_id)? {
                return Err(format!(
                    "cannot activate missing or archived board {active_board_id}"
                ));
            }
            if super::boards::board_is_non_archived(transaction, *board_id)? {
                super::boards::archive_board_rows(transaction, *board_id, *active_board_id)?;
            } else {
                super::boards::persist_active_board_id(transaction, *active_board_id)?;
            }
        }
        BoardLifecyclePersistence::Restore {
            document,
            active_board_id,
        } => {
            super::boards::restore_board_document_rows(transaction, document, *active_board_id)?;
        }
    }
    Ok(())
}
