use super::*;

pub(crate) enum BoardDurabilityOperation {
    Create(crate::data::BoardDocumentSnapshot),
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
            BoardDurabilityOperation::Create(document) => {
                persist_created_board(&transaction, document)?;
            }
            BoardDurabilityOperation::Document { board_id, changes } => {
                persist_document_changes(&transaction, *board_id, changes)?;
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

fn persist_created_board(
    transaction: &Transaction<'_>,
    document: &crate::data::BoardDocumentSnapshot,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO boards (
                id, name, archived, camera_pan_x, camera_pan_y, camera_zoom, canvas_background
             ) VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, archived = 0, camera_pan_x = excluded.camera_pan_x,
                camera_pan_y = excluded.camera_pan_y, camera_zoom = excluded.camera_zoom,
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
        .map_err(|error| format!("failed to create board {}: {error}", document.board.id))?;
    let changes = crate::canvas::CanvasDocumentChanges {
        created_placements: document.placements.clone(),
        updated_annotations: document.annotations.clone(),
        stack_order: Some(document.stack_order.clone()),
        ..Default::default()
    };
    persist_document_changes(transaction, document.board.id, &changes)
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
    for placement in &changes.created_placements {
        let content_state = match placement.content {
            crate::data::PlacementContent::Loaded { .. } => "loaded",
            crate::data::PlacementContent::Loading { .. } => "loading",
            crate::data::PlacementContent::Failed { .. } => "failed",
        };
        let content = serde_json::to_string(&placement.content)
            .map_err(|error| format!("failed to encode placement content: {error}"))?;
        transaction
            .execute(
                "INSERT INTO board_items (
                    id, board_id, asset_id, x, y, width, height, scale, rotation, z_index,
                    archived, placement_content_state, placement_lifecycle, content_resolution_intent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'committed', ?13)
                 ON CONFLICT(id) DO UPDATE SET
                    board_id = excluded.board_id, asset_id = excluded.asset_id, x = excluded.x,
                    y = excluded.y, width = excluded.width, height = excluded.height,
                    scale = excluded.scale, rotation = excluded.rotation, z_index = excluded.z_index,
                    archived = excluded.archived, placement_content_state = excluded.placement_content_state,
                    placement_lifecycle = excluded.placement_lifecycle,
                    content_resolution_intent = excluded.content_resolution_intent",
                params![
                    placement.state.id,
                    board_id,
                    placement.state.asset_id,
                    placement.state.geometry.center.x,
                    placement.state.geometry.center.y,
                    placement.state.geometry.size.x,
                    placement.state.geometry.size.y,
                    placement.state.geometry.scale,
                    placement.state.geometry.rotation,
                    placement.state.geometry.z_index,
                    if placement.state.archived { 1_i64 } else { 0_i64 },
                    content_state,
                    content,
                ],
            )
            .map_err(|error| format!("failed to create placement {}: {error}", placement.state.id))?;
    }
    for annotation in &changes.updated_annotations {
        transaction
            .execute(
                "INSERT INTO canvas_annotations (
                    id, board_id, kind, start_x, start_y, end_x, end_y, text, text_size,
                    stroke_width, color, background_enabled, archived
                 ) VALUES (?11, ?12, ?13, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    board_id = excluded.board_id, kind = excluded.kind,
                    start_x = excluded.start_x, start_y = excluded.start_y,
                    end_x = excluded.end_x, end_y = excluded.end_y, text = excluded.text,
                    text_size = excluded.text_size, stroke_width = excluded.stroke_width,
                    color = excluded.color, background_enabled = excluded.background_enabled,
                    archived = excluded.archived",
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
                    annotation.kind.as_str(),
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
