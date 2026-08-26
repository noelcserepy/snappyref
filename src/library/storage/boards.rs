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

fn archive_board_rows(
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

fn persist_active_board_id(connection: &Connection, board_id: i64) -> Result<(), String> {
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

impl Library {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_boards(&self) -> Result<Vec<Board>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, archived
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
                    archived: row.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(|error| format!("failed to read boards query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect boards: {error}"))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_board_summaries(&self) -> Result<Vec<crate::data::BoardSummary>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    boards.id,
                    boards.name,
                    boards.archived,
                    COUNT(board_items.id)
                 FROM boards
                 LEFT JOIN board_items
                   ON board_items.board_id = boards.id
                  AND board_items.archived = 0
                 WHERE boards.archived = 0
                 GROUP BY boards.id, boards.name, boards.archived
                 ORDER BY boards.id ASC",
            )
            .map_err(|error| format!("failed to prepare board summaries query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(crate::data::BoardSummary {
                    board: Board {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        archived: row.get::<_, i64>(2)? != 0,
                    },
                    placement_count: row.get::<_, i64>(3)? as usize,
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
        let connection = self.connection()?;
        let board = connection
            .query_row(
                "SELECT id, name, archived
                 FROM boards
                 WHERE id = ?1
                   AND archived = 0",
                params![board_id],
                |row| {
                    Ok(Board {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        archived: row.get::<_, i64>(2)? != 0,
                    })
                },
            )
            .map_err(|error| format!("failed to load board {board_id}: {error}"))?;
        let viewport = super::session::board_viewport_state(&connection, board_id)
            .map_err(|error| format!("failed to load board {board_id} viewport: {error}"))?;
        let placements = self.load_placements_for_board_id(&connection, board_id)?;
        let annotations =
            super::annotations::load_canvas_annotations_for_board_id(&connection, board_id)?;
        let stack_order =
            super::stack_order::load_canvas_stack_order_for_board_id(&connection, board_id)?;

        Ok(crate::data::BoardDocumentSnapshot {
            board,
            placements,
            annotations,
            stack_order,
            viewport,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn create_board(&self, name: &str) -> Result<Board, String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO boards (
                    name,
                    archived,
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    canvas_background
                 ) VALUES (?1, 0, 0.0, 0.0, ?2, 'none')",
                params![name, DEFAULT_VIEWPORT_ZOOM],
            )
            .map_err(|error| format!("failed to create board: {error}"))?;

        Ok(Board {
            id: connection.last_insert_rowid(),
            name: name.to_owned(),
            archived: false,
        })
    }

    pub fn rename_board(&self, board_id: i64, name: &str) -> Result<(), String> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Ok(());
        }

        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE boards
                 SET name = ?1
                 WHERE id = ?2
                   AND archived = 0",
                params![trimmed_name, board_id],
            )
            .map_err(|error| format!("failed to rename board {board_id}: {error}"))?;

        Ok(())
    }

    pub fn active_board_after_delete(&self, board_id: i64) -> Result<i64, String> {
        let connection = self.connection()?;
        let boards = list_non_archived_board_ids(&connection)?;
        let current_active_board_id = active_board_id(&connection)?;
        active_board_after_delete(&boards, board_id, current_active_board_id)
    }

    pub fn archive_board(&self, board_id: i64, next_active_board_id: i64) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to start board archive transaction: {error}"))?;
        if board_id == next_active_board_id {
            return Err(format!("cannot keep archived board {board_id} active"));
        }
        if !board_is_non_archived(&transaction, next_active_board_id)? {
            return Err(format!(
                "cannot activate missing or archived board {next_active_board_id}"
            ));
        }
        if board_is_non_archived(&transaction, board_id)? {
            archive_board_rows(&transaction, board_id, next_active_board_id)?;
        } else {
            persist_active_board_id(&transaction, next_active_board_id)?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit board archive transaction: {error}"))
    }

    pub fn restore_board_document(
        &self,
        snapshot: &crate::data::BoardDocumentSnapshot,
        active_board_id: i64,
    ) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to start board restore transaction: {error}"))?;
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
                 SET name = ?1,
                     archived = 0,
                     camera_pan_x = ?2,
                     camera_pan_y = ?3,
                     camera_zoom = ?4,
                     canvas_background = ?5
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
            let changed = transaction
                .execute(
                    "UPDATE board_items
                     SET asset_id = ?1,
                         x = ?2,
                         y = ?3,
                         width = ?4,
                         height = ?5,
                         scale = ?6,
                         rotation = ?7,
                         z_index = ?8,
                         archived = 0
                     WHERE id = ?9
                       AND board_id = ?10",
                    params![
                        placement.state.asset_id,
                        placement.state.geometry.center.x,
                        placement.state.geometry.center.y,
                        placement.state.geometry.size.x,
                        placement.state.geometry.size.y,
                        placement.state.geometry.scale,
                        placement.state.geometry.rotation,
                        placement.state.geometry.z_index,
                        placement.state.id,
                        snapshot.board.id,
                    ],
                )
                .map_err(|error| {
                    format!(
                        "failed to restore placement {}: {error}",
                        placement.state.id
                    )
                })?;
            if changed != 1 {
                return Err(format!(
                    "cannot restore missing placement {}",
                    placement.state.id
                ));
            }
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
        for annotation in &snapshot.annotations {
            let changed = transaction
                .execute(
                    "UPDATE canvas_annotations
                     SET kind = ?1,
                         start_x = ?2,
                         start_y = ?3,
                         end_x = ?4,
                         end_y = ?5,
                         text = ?6,
                         text_size = ?7,
                         stroke_width = ?8,
                         color = ?9,
                         background_enabled = ?10,
                         archived = 0
                     WHERE id = ?11
                       AND board_id = ?12",
                    params![
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
                        annotation.id,
                        snapshot.board.id,
                    ],
                )
                .map_err(|error| {
                    format!("failed to restore annotation {}: {error}", annotation.id)
                })?;
            if changed != 1 {
                return Err(format!(
                    "cannot restore missing annotation {}",
                    annotation.id
                ));
            }
        }

        transaction
            .execute(
                "DELETE FROM canvas_stack_items WHERE board_id = ?1",
                params![snapshot.board.id],
            )
            .map_err(|error| {
                format!(
                    "failed to reset board {} stack order: {error}",
                    snapshot.board.id
                )
            })?;
        for (stack_index, item) in snapshot.stack_order.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO canvas_stack_items (
                        board_id,
                        item_kind,
                        item_id,
                        stack_index
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        snapshot.board.id,
                        item.kind(),
                        item.item_id(),
                        stack_index as i64,
                    ],
                )
                .map_err(|error| {
                    format!(
                        "failed to restore board {} stack order: {error}",
                        snapshot.board.id
                    )
                })?;
        }

        if active_board_id != snapshot.board.id
            && !board_is_non_archived(&transaction, active_board_id)?
        {
            return Err(format!(
                "cannot activate missing or archived board {active_board_id}"
            ));
        }
        persist_active_board_id(&transaction, active_board_id)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit board restore transaction: {error}"))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_active_board(&self, board_id: i64) -> Result<(), String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!(
                "cannot activate missing or archived board {board_id}"
            ));
        }

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
}
