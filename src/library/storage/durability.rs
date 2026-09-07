use super::connection::Library;
use super::connection::write_transaction;
use rusqlite::Transaction;
use rusqlite::params;

pub(crate) enum BoardDurabilityOperation {
    Document {
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    },
    Lifecycle(crate::active_board::persistence::BoardLifecyclePersistence),
}

impl Library {
    pub(crate) fn persist_board_unit(
        &self,
        operation: &BoardDurabilityOperation,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<(), String> {
        let history_payload = history.encode()?;
        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "Board durability")?;
        self.persist_document_identity(&transaction)?;

        match operation {
            BoardDurabilityOperation::Document { board_id, changes } => {
                persist_document_changes(&transaction, *board_id, changes)?;
            }
            BoardDurabilityOperation::Lifecycle(change) => {
                persist_lifecycle_change(&transaction, change)?;
            }
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

    /// Lifecycle change without a history rewrite. Test seam for board setup;
    /// the app always goes through `persist_board_unit`.
    #[cfg(test)]
    pub(crate) fn persist_lifecycle_change(
        &self,
        change: &crate::active_board::persistence::BoardLifecyclePersistence,
    ) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "Board lifecycle")?;
        persist_lifecycle_change(&transaction, change)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit Board lifecycle transaction: {error}"))
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
    for placement in &changes.updated_placements {
        super::placements::restore_board_placement(transaction, board_id, placement)?;
    }
    for annotation in &changes.updated_annotations {
        super::annotations::upsert_board_annotation(
            transaction,
            board_id,
            annotation,
            annotation.archived,
        )?;
    }
    if let Some(groups) = &changes.groups {
        super::groups::replace_board_groups(transaction, board_id, groups)?;
    }
    if let Some(stack_order) = &changes.stack_order {
        super::stack_order::replace_board_stack_order(transaction, board_id, stack_order)?;
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
