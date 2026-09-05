use super::*;

pub(super) fn active_board_id(connection: &Connection) -> Result<i64, String> {
    let candidate = connection
        .query_row(
            "SELECT active_board_id FROM user_session WHERE id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(1);

    if board_is_non_archived(connection, candidate)? {
        return Ok(candidate);
    }

    connection
        .query_row(
            "SELECT id FROM boards WHERE archived = 0 ORDER BY id ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to load active board: {error}"))
}

#[cfg(test)]
pub(super) fn list_non_archived_board_ids(connection: &Connection) -> Result<Vec<i64>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id
             FROM boards
             WHERE archived = 0
             ORDER BY id ASC",
        )
        .map_err(|error| format!("failed to prepare board id query: {error}"))?;

    let rows = statement
        .query_map([], |row| row.get(0))
        .map_err(|error| format!("failed to read board id query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect board ids: {error}"))
}

pub(super) fn board_is_non_archived(
    connection: &Connection,
    board_id: i64,
) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM boards WHERE id = ?1 AND archived = 0",
            params![board_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect board {board_id}: {error}"))?;
    Ok(count > 0)
}

#[cfg(test)]
fn active_board_after_delete(
    boards: &[i64],
    board_id: i64,
    active_board_id: i64,
) -> Result<i64, String> {
    let Some(target_index) = boards.iter().position(|id| *id == board_id) else {
        return Err(format!(
            "cannot delete missing or archived board {board_id}"
        ));
    };
    if boards.len() == 1 {
        return Err("cannot delete the last board".to_owned());
    }
    if active_board_id != board_id {
        return Ok(active_board_id);
    }
    if target_index > 0 {
        Ok(boards[target_index - 1])
    } else {
        Ok(boards[target_index + 1])
    }
}

pub(super) fn archive_board_rows(
    transaction: &Transaction<'_>,
    board_id: i64,
    next_active_board_id: i64,
) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE board_items
             SET archived = 1
             WHERE board_id = ?1",
            params![board_id],
        )
        .map_err(|error| format!("failed to archive board {board_id} placements: {error}"))?;
    transaction
        .execute(
            "UPDATE canvas_annotations
             SET archived = 1
             WHERE board_id = ?1",
            params![board_id],
        )
        .map_err(|error| format!("failed to archive board {board_id} annotations: {error}"))?;
    transaction
        .execute(
            "UPDATE boards
             SET archived = 1
             WHERE id = ?1",
            params![board_id],
        )
        .map_err(|error| format!("failed to archive board {board_id}: {error}"))?;
    persist_active_board_id(transaction, next_active_board_id)
}

pub(super) fn persist_active_board_id(
    connection: &Connection,
    board_id: i64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO user_session (
                id,
                camera_pan_x,
                camera_pan_y,
                camera_zoom,
                active_board_id
             )
             VALUES (1, 0.0, 0.0, ?2, ?1)
             ON CONFLICT(id) DO UPDATE SET active_board_id = excluded.active_board_id",
            params![board_id, DEFAULT_VIEWPORT_ZOOM],
        )
        .map_err(|error| format!("failed to persist active board: {error}"))?;
    Ok(())
}

pub(super) fn restore_board_document_rows(
    transaction: &Transaction<'_>,
    snapshot: &crate::data::BoardDocumentSnapshot,
    active_board_id: i64,
) -> Result<(), String> {
    let board_exists: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM boards WHERE id = ?1",
            params![snapshot.board.id],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect board {}: {error}", snapshot.board.id))?;
    if board_exists == 0 {
        return Err(format!(
            "cannot restore missing board {}",
            snapshot.board.id
        ));
    }
    transaction
        .execute(
            "UPDATE boards
             SET name = ?1, archived = 0, camera_pan_x = ?2, camera_pan_y = ?3,
                 camera_zoom = ?4, canvas_background = ?5
             WHERE id = ?6",
            params![
                snapshot.board.name,
                snapshot.viewport.camera_pan_x,
                snapshot.viewport.camera_pan_y,
                snapshot.viewport.camera_zoom,
                snapshot.viewport.canvas_background.as_str(),
                snapshot.board.id,
            ],
        )
        .map_err(|error| format!("failed to restore board {}: {error}", snapshot.board.id))?;
    transaction
        .execute(
            "UPDATE board_items SET archived = 1 WHERE board_id = ?1",
            params![snapshot.board.id],
        )
        .map_err(|error| {
            format!(
                "failed to reset board {} placements: {error}",
                snapshot.board.id
            )
        })?;
    for placement in &snapshot.placements {
        super::placements::restore_board_placement(transaction, snapshot.board.id, placement)?;
    }
    transaction
        .execute(
            "UPDATE canvas_annotations SET archived = 1 WHERE board_id = ?1",
            params![snapshot.board.id],
        )
        .map_err(|error| {
            format!(
                "failed to reset board {} annotations: {error}",
                snapshot.board.id
            )
        })?;
    super::groups::replace_board_groups(transaction, snapshot.board.id, &snapshot.groups)?;
    for annotation in &snapshot.annotations {
        super::annotations::upsert_board_annotation(
            transaction,
            snapshot.board.id,
            annotation,
            false,
        )?;
    }
    super::stack_order::replace_board_stack_order(
        transaction,
        snapshot.board.id,
        &snapshot.stack_order,
    )?;
    if active_board_id != snapshot.board.id && !board_is_non_archived(transaction, active_board_id)?
    {
        return Err(format!(
            "cannot activate missing or archived board {active_board_id}"
        ));
    }
    persist_active_board_id(transaction, active_board_id)
}

impl Library {
    pub(crate) fn new_board(&self, name: String) -> Board {
        Board {
            id: self.allocate_document_id(),
            name,
        }
    }

    #[cfg(test)]
    pub fn list_boards(&self) -> Result<Vec<Board>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name
                 FROM boards
                 WHERE archived = 0
                 ORDER BY id ASC",
            )
            .map_err(|error| format!("failed to prepare boards query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(Board {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })
            .map_err(|error| format!("failed to read boards query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect boards: {error}"))
    }

    pub fn list_board_summaries(&self) -> Result<Vec<crate::data::BoardSummary>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    boards.id,
                    boards.name,
                    COUNT(board_items.id)
                 FROM boards
                 LEFT JOIN board_items
                   ON board_items.board_id = boards.id
                  AND board_items.archived = 0
                 WHERE boards.archived = 0
                 GROUP BY boards.id, boards.name
                 ORDER BY boards.id ASC",
            )
            .map_err(|error| format!("failed to prepare board summaries query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(crate::data::BoardSummary {
                    board: Board {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    },
                    placement_count: row.get::<_, i64>(2)? as usize,
                })
            })
            .map_err(|error| format!("failed to read board summaries query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board summaries: {error}"))
    }

    pub fn load_board_document(
        &self,
        board_id: i64,
    ) -> Result<crate::data::BoardDocumentSnapshot, String> {
        let mut connection = self.connection()?;
        // Read-only snapshot: a deferred transaction gives a consistent view
        // without contending for the write lock.
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start board snapshot transaction: {error}"))?;
        let board = transaction
            .query_row(
                "SELECT id, name
                 FROM boards
                 WHERE id = ?1
                   AND archived = 0",
                params![board_id],
                |row| {
                    Ok(Board {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .map_err(|error| format!("failed to load board {board_id}: {error}"))?;
        let viewport = super::session::board_viewport_state(&transaction, board_id)
            .map_err(|error| format!("failed to load board {board_id} viewport: {error}"))?;
        let placements = self.load_placements_for_board_id(&transaction, board_id)?;
        let annotations =
            super::annotations::load_canvas_annotations_for_board_id(&transaction, board_id)?;
        let stack_order =
            super::stack_order::load_canvas_stack_order_for_board_id(&transaction, board_id)?;

        let groups = super::groups::load_groups_for_board_id(&transaction, board_id)?;
        let snapshot = crate::data::BoardDocumentSnapshot {
            board,
            placements,
            annotations,
            groups,
            stack_order,
            viewport,
        };
        transaction
            .commit()
            .map_err(|error| format!("failed to finish board snapshot transaction: {error}"))?;
        Ok(snapshot)
    }

    #[cfg(test)]
    pub fn create_board(&self, name: &str) -> Result<Board, String> {
        let connection = self.connection()?;
        let board_id = self.allocate_document_id();
        connection
            .execute(
                "INSERT INTO boards (
                    id,
                    name,
                    archived,
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    canvas_background
                 ) VALUES (?1, ?2, 0, 0.0, 0.0, ?3, 'none')",
                params![board_id, name, DEFAULT_VIEWPORT_ZOOM],
            )
            .map_err(|error| format!("failed to create board: {error}"))?;

        Ok(Board {
            id: board_id,
            name: name.to_owned(),
        })
    }

    #[cfg(test)]
    pub fn rename_board(&self, board_id: i64, name: &str) -> Result<(), String> {
        use crate::active_board::persistence::BoardLifecyclePersistence;
        self.persist_lifecycle_change(&BoardLifecyclePersistence::Rename {
            board_id,
            name: name.to_owned(),
        })
    }

    #[cfg(test)]
    pub fn active_board_after_delete(&self, board_id: i64) -> Result<i64, String> {
        let connection = self.connection()?;
        let boards = list_non_archived_board_ids(&connection)?;
        let current_active_board_id = active_board_id(&connection)?;
        active_board_after_delete(&boards, board_id, current_active_board_id)
    }

    #[cfg(test)]
    pub fn archive_board(&self, board_id: i64, next_active_board_id: i64) -> Result<(), String> {
        use crate::active_board::persistence::BoardLifecyclePersistence;
        self.persist_lifecycle_change(&BoardLifecyclePersistence::Archive {
            board_id,
            active_board_id: next_active_board_id,
        })
    }

    #[cfg(test)]
    pub fn restore_board_document(
        &self,
        snapshot: &crate::data::BoardDocumentSnapshot,
        active_board_id: i64,
    ) -> Result<(), String> {
        use crate::active_board::persistence::BoardLifecyclePersistence;
        self.persist_lifecycle_change(&BoardLifecyclePersistence::Restore {
            document: snapshot.clone(),
            active_board_id,
        })
    }

    pub fn set_active_board(&self, board_id: i64) -> Result<(), String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!(
                "cannot activate missing or archived board {board_id}"
            ));
        }
        persist_active_board_id(&connection, board_id)
    }
}
