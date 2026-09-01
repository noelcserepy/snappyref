use super::*;

pub(crate) enum BoardDurabilityOperation {
    Document {
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
    },
    Lifecycle(crate::active_board::persistence::BoardLifecyclePersistence),
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
                persist_document_changes(&transaction, *board_id, changes)?;
            }
            BoardDurabilityOperation::Lifecycle(change) => {
                persist_lifecycle_change(&transaction, change)?;
            }
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
) -> Result<(), String> {
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
        let loaded_content = item
            .asset_id
            .map(|asset_id| {
                serde_json::to_string(&crate::data::PlacementContent::Loaded { asset_id })
            })
            .transpose()
            .map_err(|error| format!("failed to encode placement content: {error}"))?;
        transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1, x = ?2, y = ?3, width = ?4, height = ?5,
                     scale = ?6, rotation = ?7, z_index = ?8, archived = ?9,
                     placement_content_state = CASE WHEN ?1 IS NOT NULL THEN 'loaded' ELSE placement_content_state END,
                     content_resolution_intent = CASE WHEN ?1 IS NOT NULL THEN ?10 ELSE content_resolution_intent END
                 WHERE id = ?11 AND board_id = ?12",
                params![
                    item.asset_id,
                    item.geometry.center.x,
                    item.geometry.center.y,
                    item.geometry.size.x,
                    item.geometry.size.y,
                    item.geometry.scale,
                    item.geometry.rotation,
                    item.geometry.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    loaded_content,
                    item.id,
                    board_id,
                ],
            )
            .map_err(|error| format!("failed to update placement document state: {error}"))?;
    }
    for annotation in &changes.updated_annotations {
        transaction
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
