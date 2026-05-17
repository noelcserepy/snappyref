use super::session::{StoredUserSessionPayload, legacy_user_session_from_row};
use super::stack_order::ensure_canvas_stack_items_populated;
use super::*;

const DEFAULT_VIEWPORT_ZOOM_SQL: &str = "0.45";

fn ensure_board_items_archived_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(board_items)")
        .map_err(|error| format!("failed to inspect board_items schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read board_items schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect board_items schema: {error}"))?;

    if columns.iter().any(|column| column == "archived") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE board_items ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| format!("failed to add archived column to board_items: {error}"))?;

    Ok(())
}

fn ensure_board_items_placement_columns(connection: &Connection) -> Result<(), String> {
    ensure_column(
        connection,
        "board_items",
        "placement_content_state",
        "ALTER TABLE board_items ADD COLUMN placement_content_state TEXT NOT NULL DEFAULT 'loaded'",
    )?;
    ensure_column(
        connection,
        "board_items",
        "placement_lifecycle",
        "ALTER TABLE board_items ADD COLUMN placement_lifecycle TEXT NOT NULL DEFAULT 'committed'",
    )?;
    ensure_column(
        connection,
        "board_items",
        "visual_width",
        "ALTER TABLE board_items ADD COLUMN visual_width REAL",
    )?;
    ensure_column(
        connection,
        "board_items",
        "visual_height",
        "ALTER TABLE board_items ADD COLUMN visual_height REAL",
    )?;
    ensure_column(
        connection,
        "board_items",
        "content_resolution_intent",
        "ALTER TABLE board_items ADD COLUMN content_resolution_intent TEXT",
    )
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), String> {
    if table_has_column(connection, table, column)? {
        return Ok(());
    }
    connection
        .execute(alter_sql, [])
        .map_err(|error| format!("failed to add {column} column to {table}: {error}"))?;
    Ok(())
}

fn ensure_assets_archived_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "assets", "archived")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE assets ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| format!("failed to add archived column to assets: {error}"))?;

    Ok(())
}

fn ensure_default_board(connection: &Connection) -> Result<(), String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM boards WHERE archived = 0",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to count boards: {error}"))?;

    if count == 0 {
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
                 ) VALUES (1, 'My Board', 0, 0.0, 0.0, ?1, 'none')",
                params![DEFAULT_VIEWPORT_ZOOM],
            )
            .map_err(|error| format!("failed to create default board: {error}"))?;
    }

    Ok(())
}

fn ensure_board_items_board_id_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "board_items", "board_id")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE board_items ADD COLUMN board_id INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .map_err(|error| format!("failed to add board_id column to board_items: {error}"))?;

    Ok(())
}

fn ensure_canvas_annotations_board_id_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "canvas_annotations", "board_id")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE canvas_annotations ADD COLUMN board_id INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .map_err(|error| format!("failed to add board_id column to canvas_annotations: {error}"))?;

    Ok(())
}

fn ensure_user_session_active_board_id_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "user_session", "active_board_id")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN active_board_id INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .map_err(|error| {
            format!("failed to add active_board_id column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_sidebar_open_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "user_session", "sidebar_open")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN sidebar_open INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| format!("failed to add sidebar_open column to user_session: {error}"))?;

    Ok(())
}

fn ensure_user_session_sidebar_tab_column(connection: &Connection) -> Result<(), String> {
    if table_has_column(connection, "user_session", "sidebar_tab")? {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN sidebar_tab TEXT NOT NULL DEFAULT 'settings'",
            [],
        )
        .map_err(|error| format!("failed to add sidebar_tab column to user_session: {error}"))?;

    Ok(())
}

fn ensure_user_session_payload_column(connection: &Connection) -> Result<(), String> {
    if !table_has_column(connection, "user_session", "session_payload")? {
        connection
            .execute(
                "ALTER TABLE user_session ADD COLUMN session_payload TEXT",
                [],
            )
            .map_err(|error| {
                format!("failed to add session_payload column to user_session: {error}")
            })?;
    }

    backfill_user_session_payload(connection)
}

fn backfill_user_session_payload(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT
                auto_image_scaling,
                import_gap,
                import_target_canvas_area,
                semantic_search_enabled,
                canvas_image_max_side,
                rotation_snap_degrees,
                default_textbox_background,
                sidebar_open,
                sidebar_tab,
                session_payload
             FROM user_session
             WHERE id = 1",
        )
        .map_err(|error| format!("failed to prepare user-session payload backfill: {error}"))?;

    let payload = statement.query_row([], |row| {
        let existing = row.get::<_, Option<String>>(9).ok().flatten();
        if existing
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
        {
            return Ok(None);
        }

        let legacy = legacy_user_session_from_row(row, 1, ViewportState::default())?;
        Ok(Some(StoredUserSessionPayload::from_session(&legacy)))
    });

    let payload = match payload {
        Ok(payload) => payload,
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(format!("failed to backfill user session payload: {error}")),
    };

    let Some(payload) = payload else {
        return Ok(());
    };
    let payload_json = serde_json::to_string(&payload)
        .map_err(|error| format!("failed to encode user session payload backfill: {error}"))?;
    connection
        .execute(
            "UPDATE user_session
             SET session_payload = ?1
             WHERE id = 1",
            params![payload_json],
        )
        .map_err(|error| format!("failed to save user session payload backfill: {error}"))?;

    Ok(())
}

fn migrate_default_board_viewport(connection: &Connection) -> Result<(), String> {
    let session = connection.query_row(
        "SELECT camera_pan_x, camera_pan_y, camera_zoom, canvas_background
         FROM user_session
         WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, f32>(0)?,
                row.get::<_, f32>(1)?,
                row.get::<_, f32>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    );

    let Ok((pan_x, pan_y, zoom, background)) = session else {
        return Ok(());
    };

    connection
        .execute(
            "UPDATE boards
             SET camera_pan_x = ?1,
                 camera_pan_y = ?2,
                 camera_zoom = ?3,
                 canvas_background = ?4
             WHERE id = 1
               AND name = 'My Board'",
            params![pan_x, pan_y, zoom, background],
        )
        .map_err(|error| format!("failed to migrate default board viewport: {error}"))?;

    Ok(())
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("failed to inspect {table} schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read {table} schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect {table} schema: {error}"))?;

    Ok(columns.iter().any(|existing| existing == column))
}

fn ensure_asset_embeddings_failure_columns(connection: &Connection) -> Result<(), String> {
    if !table_has_column(connection, "asset_embeddings", "failure_kind")? {
        connection
            .execute(
                "ALTER TABLE asset_embeddings ADD COLUMN failure_kind TEXT",
                [],
            )
            .map_err(|error| format!("failed to add embedding failure_kind column: {error}"))?;
    }
    if !table_has_column(connection, "asset_embeddings", "failure_count")? {
        connection
            .execute(
                "ALTER TABLE asset_embeddings
                 ADD COLUMN failure_count INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|error| format!("failed to add embedding failure_count column: {error}"))?;
    }
    if !table_has_column(connection, "asset_embeddings", "next_retry_at")? {
        connection
            .execute(
                "ALTER TABLE asset_embeddings ADD COLUMN next_retry_at INTEGER",
                [],
            )
            .map_err(|error| format!("failed to add embedding next_retry_at column: {error}"))?;
    }
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM sqlite_master
             WHERE type = 'table'
               AND name = ?1",
            params![table],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect {table} table: {error}"))?;
    Ok(count > 0)
}

fn ensure_canvas_annotations_text_size_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(canvas_annotations)")
        .map_err(|error| format!("failed to inspect canvas_annotations schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read canvas_annotations schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas_annotations schema: {error}"))?;

    if columns.iter().any(|column| column == "text_size") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE canvas_annotations ADD COLUMN text_size TEXT NOT NULL DEFAULT 'm'",
            [],
        )
        .map_err(|error| {
            format!("failed to add text_size column to canvas_annotations: {error}")
        })?;

    Ok(())
}

fn ensure_canvas_annotations_stroke_width_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(canvas_annotations)")
        .map_err(|error| format!("failed to inspect canvas_annotations schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read canvas_annotations schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas_annotations schema: {error}"))?;

    if columns.iter().any(|column| column == "stroke_width") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE canvas_annotations ADD COLUMN stroke_width TEXT NOT NULL DEFAULT 'm'",
            [],
        )
        .map_err(|error| {
            format!("failed to add stroke_width column to canvas_annotations: {error}")
        })?;

    Ok(())
}

fn ensure_canvas_annotations_color_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(canvas_annotations)")
        .map_err(|error| format!("failed to inspect canvas_annotations schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read canvas_annotations schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas_annotations schema: {error}"))?;

    if columns.iter().any(|column| column == "color") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE canvas_annotations ADD COLUMN color TEXT NOT NULL DEFAULT 'default'",
            [],
        )
        .map_err(|error| format!("failed to add color column to canvas_annotations: {error}"))?;

    Ok(())
}

fn ensure_canvas_annotations_background_enabled_column(
    connection: &Connection,
) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(canvas_annotations)")
        .map_err(|error| format!("failed to inspect canvas_annotations schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read canvas_annotations schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect canvas_annotations schema: {error}"))?;

    if columns.iter().any(|column| column == "background_enabled") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE canvas_annotations ADD COLUMN background_enabled INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| {
            format!("failed to add background_enabled column to canvas_annotations: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_auto_image_scaling_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns.iter().any(|column| column == "auto_image_scaling") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN auto_image_scaling INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .map_err(|error| {
            format!("failed to add auto_image_scaling column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_import_gap_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns.iter().any(|column| column == "import_gap") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN import_gap REAL NOT NULL DEFAULT 0.0",
            [],
        )
        .map_err(|error| format!("failed to add import_gap column to user_session: {error}"))?;

    Ok(())
}

fn ensure_user_session_import_target_canvas_area_column(
    connection: &Connection,
) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns
        .iter()
        .any(|column| column == "import_target_canvas_area")
    {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN import_target_canvas_area REAL NOT NULL DEFAULT 90000.0",
            [],
        )
        .map_err(|error| {
            format!("failed to add import_target_canvas_area column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_canvas_background_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns.iter().any(|column| column == "canvas_background") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN canvas_background TEXT NOT NULL DEFAULT 'none'",
            [],
        )
        .map_err(|error| {
            format!("failed to add canvas_background column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_semantic_search_enabled_column(
    connection: &Connection,
) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns
        .iter()
        .any(|column| column == "semantic_search_enabled")
    {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN semantic_search_enabled INTEGER NOT NULL DEFAULT 1",
            [],
        )
        .map_err(|error| {
            format!("failed to add semantic_search_enabled column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_canvas_image_mode_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns.iter().any(|column| column == "canvas_image_mode") {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN canvas_image_mode TEXT NOT NULL DEFAULT 'downscale'",
            [],
        )
        .map_err(|error| format!("failed to add canvas_image_mode column: {error}"))?;

    Ok(())
}

fn ensure_user_session_canvas_image_max_side_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns
        .iter()
        .any(|column| column == "canvas_image_max_side")
    {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN canvas_image_max_side INTEGER NOT NULL DEFAULT 2000",
            [],
        )
        .map_err(|error| format!("failed to add canvas_image_max_side column: {error}"))?;

    Ok(())
}

fn ensure_user_session_rotation_snap_degrees_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns
        .iter()
        .any(|column| column == "rotation_snap_degrees")
    {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN rotation_snap_degrees REAL NOT NULL DEFAULT 15.0",
            [],
        )
        .map_err(|error| {
            format!("failed to add rotation_snap_degrees column to user_session: {error}")
        })?;

    Ok(())
}

fn ensure_user_session_default_textbox_background_column(
    connection: &Connection,
) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(user_session)")
        .map_err(|error| format!("failed to inspect user_session schema: {error}"))?;

    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read user_session schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to collect user_session schema: {error}"))?;

    if columns
        .iter()
        .any(|column| column == "default_textbox_background")
    {
        return Ok(());
    }

    connection
        .execute(
            "ALTER TABLE user_session ADD COLUMN default_textbox_background INTEGER NOT NULL DEFAULT 0",
            [],
        )
        .map_err(|error| {
            format!("failed to add default_textbox_background column to user_session: {error}")
        })?;

    Ok(())
}

impl Library {
    pub(super) fn initialize_schema(&self) -> Result<(), String> {
        let connection = self.connection()?;
        let should_migrate_default_board_viewport = !table_exists(&connection, "boards")?;
        connection
            .execute_batch(&format!(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS assets (
                    id            INTEGER PRIMARY KEY,
                    original_path TEXT NOT NULL UNIQUE,
                    filename      TEXT NOT NULL,
                    mime_type     TEXT NOT NULL,
                    width         INTEGER NOT NULL,
                    height        INTEGER NOT NULL,
                    file_size     INTEGER NOT NULL,
                    imported_at   INTEGER NOT NULL,
                    archived      INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS boards (
                    id            INTEGER PRIMARY KEY,
                    name          TEXT NOT NULL,
                    archived      INTEGER NOT NULL DEFAULT 0,
                    camera_pan_x  REAL NOT NULL DEFAULT 0.0,
                    camera_pan_y  REAL NOT NULL DEFAULT 0.0,
                    camera_zoom   REAL NOT NULL DEFAULT {DEFAULT_VIEWPORT_ZOOM_SQL},
                    canvas_background TEXT NOT NULL DEFAULT 'none'
                );

                CREATE TABLE IF NOT EXISTS board_items (
                    id        INTEGER PRIMARY KEY,
                    board_id  INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
                    asset_id  INTEGER REFERENCES assets(id) ON DELETE CASCADE,
                    x         REAL NOT NULL,
                    y         REAL NOT NULL,
                    scale     REAL NOT NULL,
                    rotation  REAL NOT NULL,
                    z_index   INTEGER NOT NULL,
                    archived  INTEGER NOT NULL DEFAULT 0,
                    placement_content_state TEXT NOT NULL DEFAULT 'loaded',
                    placement_lifecycle TEXT NOT NULL DEFAULT 'committed',
                    visual_width REAL,
                    visual_height REAL,
                    content_resolution_intent TEXT
                );

                CREATE TABLE IF NOT EXISTS user_session (
                    id            INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x  REAL NOT NULL,
                    camera_pan_y  REAL NOT NULL,
                    camera_zoom   REAL NOT NULL,
                    auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                    import_gap    REAL NOT NULL DEFAULT 0.0,
                    import_target_canvas_area REAL NOT NULL DEFAULT 90000.0,
                    canvas_background TEXT NOT NULL DEFAULT 'none',
                    semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                    canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                    canvas_image_max_side INTEGER NOT NULL DEFAULT 2000,
                    rotation_snap_degrees REAL NOT NULL DEFAULT 15.0,
                    default_textbox_background INTEGER NOT NULL DEFAULT 0,
                    sidebar_open INTEGER NOT NULL DEFAULT 0,
                    sidebar_tab TEXT NOT NULL DEFAULT 'settings',
                    session_payload TEXT,
                    active_board_id INTEGER NOT NULL DEFAULT 1
                );

                CREATE TABLE IF NOT EXISTS canvas_annotations (
                    id        INTEGER PRIMARY KEY,
                    board_id  INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
                    kind      TEXT NOT NULL,
                    start_x   REAL NOT NULL,
                    start_y   REAL NOT NULL,
                    end_x     REAL NOT NULL,
                    end_y     REAL NOT NULL,
                    text      TEXT NOT NULL DEFAULT '',
                    text_size TEXT NOT NULL DEFAULT 'm',
                    stroke_width TEXT NOT NULL DEFAULT 'm',
                    color TEXT NOT NULL DEFAULT 'default',
                    background_enabled INTEGER NOT NULL DEFAULT 0,
                    archived  INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS canvas_stack_items (
                    board_id    INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
                    item_kind   TEXT NOT NULL,
                    item_id     INTEGER NOT NULL,
                    stack_index INTEGER NOT NULL,
                    PRIMARY KEY (board_id, item_kind, item_id)
                );

                CREATE TABLE IF NOT EXISTS asset_embeddings (
                    asset_id        INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    model_key       TEXT NOT NULL,
                    dimension       INTEGER NOT NULL,
                    status          TEXT NOT NULL,
                    last_error      TEXT,
                    failure_kind    TEXT,
                    failure_count   INTEGER NOT NULL DEFAULT 0,
                    next_retry_at   INTEGER,
                    embedding_blob  BLOB,
                    updated_at      INTEGER NOT NULL,
                    PRIMARY KEY (asset_id, model_key),
                    CHECK (status IN ('pending', 'ready', 'failed')),
                    CHECK (failure_kind IS NULL OR failure_kind IN ('retryable', 'terminal'))
                );

                CREATE INDEX IF NOT EXISTS idx_asset_embeddings_model_status
                ON asset_embeddings(model_key, status);
                ",
            ))
            .map_err(|error| format!("failed to initialize database schema: {error}"))?;

        ensure_board_items_archived_column(&connection)?;
        ensure_board_items_placement_columns(&connection)?;
        ensure_assets_archived_column(&connection)?;
        ensure_default_board(&connection)?;
        ensure_board_items_board_id_column(&connection)?;
        ensure_user_session_auto_image_scaling_column(&connection)?;
        ensure_user_session_import_gap_column(&connection)?;
        ensure_user_session_import_target_canvas_area_column(&connection)?;
        ensure_user_session_canvas_background_column(&connection)?;
        ensure_user_session_semantic_search_enabled_column(&connection)?;
        ensure_user_session_canvas_image_mode_column(&connection)?;
        ensure_user_session_canvas_image_max_side_column(&connection)?;
        ensure_user_session_rotation_snap_degrees_column(&connection)?;
        ensure_user_session_default_textbox_background_column(&connection)?;
        ensure_asset_embeddings_failure_columns(&connection)?;
        ensure_user_session_sidebar_open_column(&connection)?;
        ensure_user_session_sidebar_tab_column(&connection)?;
        ensure_user_session_active_board_id_column(&connection)?;
        ensure_user_session_payload_column(&connection)?;
        ensure_canvas_annotations_text_size_column(&connection)?;
        ensure_canvas_annotations_stroke_width_column(&connection)?;
        ensure_canvas_annotations_color_column(&connection)?;
        ensure_canvas_annotations_background_enabled_column(&connection)?;
        ensure_canvas_annotations_board_id_column(&connection)?;
        ensure_canvas_stack_items_populated(&connection)?;
        if should_migrate_default_board_viewport {
            migrate_default_board_viewport(&connection)?;
        }
        Ok(())
    }
}
