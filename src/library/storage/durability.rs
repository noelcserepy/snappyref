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
        let history_rows = history.encode_owners()?;
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
        for (board_id, payload) in history_rows {
            transaction
                .execute(
                    "INSERT INTO board_history (board_id, payload) VALUES (?1, ?2)
                 ON CONFLICT(board_id) DO UPDATE SET payload = excluded.payload",
                    params![board_id, payload],
                )
                .map_err(|error| format!("failed to persist Board history: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit Board durability transaction: {error}"))
    }

    #[cfg(test)]
    pub(crate) fn fixture_board_document(
        &self,
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<(), String> {
        self.persist_board_unit(
            &BoardDurabilityOperation::Document { board_id, changes },
            &Default::default(),
        )
    }

    pub(crate) fn load_document_history(
        &self,
    ) -> Result<crate::active_board::history::DocumentHistory, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT board_id, payload FROM board_history")
            .map_err(|error| format!("failed to load Board history: {error}"))?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| format!("failed to query Board history: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read Board history: {error}"))?;
        drop(statement);
        drop(connection);
        crate::active_board::history::DocumentHistory::decode_owners(rows, self)
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
            // The restore owns every board column; the insert only makes the row exist.
            transaction
                .execute(
                    "INSERT INTO boards (id, name) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
                    params![document.board.id, document.board.name],
                )
                .map_err(|error| {
                    format!("failed to create board {}: {error}", document.board.id)
                })?;
            super::boards::restore_board_document_rows(transaction, document, *active_board_id)?;
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

pub(super) fn remap_board_history_assets(
    transaction: &Transaction<'_>,
    remaps: &std::collections::HashMap<i64, i64>,
) -> Result<(), String> {
    fn remap(value: &mut serde_json::Value, remaps: &std::collections::HashMap<i64, i64>) -> bool {
        let mut changed = false;
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(id) = fields.get_mut("asset_id")
                    && let Some(replacement) = id.as_i64().and_then(|id| remaps.get(&id))
                {
                    *id = (*replacement).into();
                    changed = true;
                }
                for child in fields.values_mut() {
                    changed |= remap(child, remaps);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    changed |= remap(child, remaps);
                }
            }
            _ => {}
        }
        changed
    }
    let mut statement = transaction
        .prepare("SELECT board_id, payload FROM board_history")
        .map_err(|error| format!("failed to query history for Asset dedupe: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("failed to read history for Asset dedupe: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect history for Asset dedupe: {error}"))?;
    drop(statement);
    for (board_id, payload) in rows {
        let mut value = serde_json::from_str(&payload)
            .map_err(|error| format!("failed to decode history for Asset dedupe: {error}"))?;
        if remap(&mut value, remaps) {
            transaction
                .execute(
                    "UPDATE board_history SET payload = ?1 WHERE board_id = ?2",
                    params![value.to_string(), board_id],
                )
                .map_err(|error| format!("failed to remap history for Asset dedupe: {error}"))?;
        }
    }
    Ok(())
}
