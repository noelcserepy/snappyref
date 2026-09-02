use super::boards::active_board_id;
use super::*;

pub(super) fn load_canvas_stack_order_for_board_id(
    connection: &Connection,
    board_id: i64,
) -> Result<Vec<CanvasStackItem>, String> {
    let mut statement = connection
        .prepare(
            "SELECT item_kind, item_id
             FROM canvas_stack_items
             WHERE board_id = ?1
               AND (
                   (
                       item_kind = 'placement'
                       AND EXISTS (
                           SELECT 1
                           FROM board_items
                           LEFT JOIN assets ON assets.id = board_items.asset_id
                           WHERE board_items.id = canvas_stack_items.item_id
                             AND board_items.board_id = canvas_stack_items.board_id
                             AND board_items.archived = 0
                             AND (assets.id IS NULL OR assets.archived = 0)
                       )
                   )
                   OR (
                       item_kind = 'annotation'
                       AND EXISTS (
                           SELECT 1
                           FROM canvas_annotations
                           WHERE canvas_annotations.id = canvas_stack_items.item_id
                             AND canvas_annotations.board_id = canvas_stack_items.board_id
                             AND canvas_annotations.archived = 0
                       )
                   )
               )
             ORDER BY stack_index ASC, item_kind ASC, item_id ASC",
        )
        .map_err(|error| format!("failed to prepare canvas-stack query: {error}"))?;

    let rows = statement
        .query_map(params![board_id], |row| {
            let kind = row.get::<_, String>(0)?;
            let item_id = row.get::<_, i64>(1)?;
            CanvasStackItem::from_parts(&kind, item_id).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("unknown canvas stack item kind: {kind}"),
                    )),
                )
            })
        })
        .map_err(|error| format!("failed to read canvas-stack query: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas-stack items: {error}"))
}

#[cfg(test)]
pub(super) fn append_canvas_stack_item(
    connection: &Connection,
    board_id: i64,
    item: CanvasStackItem,
) -> Result<(), String> {
    let next_index = next_canvas_stack_index(connection, board_id)?;
    connection
        .execute(
            "INSERT OR REPLACE INTO canvas_stack_items (
                board_id,
                item_kind,
                item_id,
                stack_index
             ) VALUES (?1, ?2, ?3, ?4)",
            params![board_id, item.kind(), item.item_id(), next_index],
        )
        .map_err(|error| format!("failed to append canvas stack item: {error}"))?;
    Ok(())
}

#[cfg(test)]
fn next_canvas_stack_index(connection: &Connection, board_id: i64) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(stack_index) + 1, 0)
             FROM canvas_stack_items
             WHERE board_id = ?1",
            params![board_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect canvas stack index: {error}"))
}

pub(super) fn ensure_canvas_stack_items_populated(connection: &Connection) -> Result<(), String> {
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM canvas_stack_items", [], |row| {
            row.get(0)
        })
        .map_err(|error| format!("failed to inspect canvas stack items: {error}"))?;
    if count > 0 {
        return Ok(());
    }

    let board_ids = {
        let mut statement = connection
            .prepare("SELECT id FROM boards WHERE archived = 0 ORDER BY id ASC")
            .map_err(|error| format!("failed to prepare board query: {error}"))?;
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("failed to read board query: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect boards: {error}"))?
    };

    for board_id in board_ids {
        let placements = {
            let mut statement = connection
                .prepare(
                    "SELECT id
                     FROM board_items
                     WHERE board_id = ?1
                       AND archived = 0
                     ORDER BY z_index ASC, id ASC",
                )
                .map_err(|error| format!("failed to prepare stack placement query: {error}"))?;
            statement
                .query_map(params![board_id], |row| row.get::<_, i64>(0))
                .map_err(|error| format!("failed to read stack placement query: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to collect stack placements: {error}"))?
        };

        let annotations = {
            let mut statement = connection
                .prepare(
                    "SELECT id
                     FROM canvas_annotations
                     WHERE board_id = ?1
                       AND archived = 0
                     ORDER BY id ASC",
                )
                .map_err(|error| format!("failed to prepare stack annotation query: {error}"))?;
            statement
                .query_map(params![board_id], |row| row.get::<_, i64>(0))
                .map_err(|error| format!("failed to read stack annotation query: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to collect stack annotations: {error}"))?
        };

        for (stack_index, item) in placements
            .into_iter()
            .map(CanvasStackItem::Placement)
            .chain(annotations.into_iter().map(CanvasStackItem::Annotation))
            .enumerate()
        {
            connection
                .execute(
                    "INSERT INTO canvas_stack_items (
                        board_id,
                        item_kind,
                        item_id,
                        stack_index
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![board_id, item.kind(), item.item_id(), stack_index as i64],
                )
                .map_err(|error| format!("failed to migrate canvas stack item: {error}"))?;
        }
    }

    Ok(())
}

impl Library {
    pub fn load_canvas_stack_order(&self) -> Result<Vec<CanvasStackItem>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        load_canvas_stack_order_for_board_id(&connection, active_board_id)
    }

    #[cfg(test)]
    pub fn save_canvas_stack_order(&self, stack_order: &[CanvasStackItem]) -> Result<(), String> {
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start canvas-stack transaction: {error}"))?;
        transaction
            .execute(
                "DELETE FROM canvas_stack_items
                 WHERE board_id = ?1",
                params![active_board_id],
            )
            .map_err(|error| format!("failed to clear canvas stack order: {error}"))?;
        for (stack_index, item) in stack_order.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO canvas_stack_items (
                        board_id,
                        item_kind,
                        item_id,
                        stack_index
                     ) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        active_board_id,
                        item.kind(),
                        item.item_id(),
                        stack_index as i64
                    ],
                )
                .map_err(|error| format!("failed to save canvas stack order: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit canvas stack order: {error}"))?;
        Ok(())
    }
}
