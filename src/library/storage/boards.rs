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

    pub fn delete_board(&self, board_id: i64) -> Result<i64, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start board delete transaction: {error}"))?;

        let boards = list_non_archived_board_ids(&transaction)?;
        let Some(target_index) = boards.iter().position(|id| *id == board_id) else {
            return Err(format!(
                "cannot delete missing or archived board {board_id}"
            ));
        };
        if boards.len() == 1 {
            return Err("cannot delete the last board".to_owned());
        }

        let current_active_board_id = active_board_id(&transaction)?;
        let remaining = boards
            .iter()
            .copied()
            .filter(|id| *id != board_id)
            .collect::<Vec<_>>();
        let next_active_board_id = if current_active_board_id == board_id {
            if target_index > 0 {
                boards[target_index - 1]
            } else {
                boards[target_index + 1]
            }
        } else {
            current_active_board_id
        };
        if !remaining.contains(&next_active_board_id) {
            return Err(format!(
                "cannot keep active board {next_active_board_id} after deleting board {board_id}"
            ));
        }

        transaction
            .execute(
                "UPDATE board_items
                 SET archived = 1
                 WHERE board_id = ?1",
                params![board_id],
            )
            .map_err(|error| format!("failed to archive board {board_id} items: {error}"))?;
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
        transaction
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
                params![next_active_board_id, DEFAULT_VIEWPORT_ZOOM],
            )
            .map_err(|error| format!("failed to persist active board: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit board delete transaction: {error}"))?;

        Ok(next_active_board_id)
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
