use super::connection::Library;
use super::session::StoredUserSessionPayload;
use super::stack_order::ensure_canvas_stack_items_populated;
use crate::data::DEFAULT_VIEWPORT_ZOOM;
use crate::data::UserSession;
use crate::data::WorkspaceSettings;
use rusqlite::Connection;
use rusqlite::params;

const DEFAULT_VIEWPORT_ZOOM_SQL: &str = "0.45";

/// Columns added after the tables first shipped, as `(table, column,
/// definition)`. New databases get them from `CREATE TABLE`; older ones are
/// upgraded in this order on open. Superseded `user_session` columns stay so
/// old rows keep loading through `legacy_user_session_from_row`.
const ADDED_COLUMNS: &[(&str, &str, &str)] = &[
    ("board_items", "archived", "INTEGER NOT NULL DEFAULT 0"),
    (
        "board_items",
        "placement_content_state",
        "TEXT NOT NULL DEFAULT 'loaded'",
    ),
    (
        "board_items",
        "placement_lifecycle",
        "TEXT NOT NULL DEFAULT 'committed'",
    ),
    ("board_items", "width", "REAL NOT NULL DEFAULT 1.0"),
    ("board_items", "height", "REAL NOT NULL DEFAULT 1.0"),
    ("board_items", "content_resolution_intent", "TEXT"),
    ("board_items", "board_id", "INTEGER NOT NULL DEFAULT 1"),
    ("assets", "archived", "INTEGER NOT NULL DEFAULT 0"),
    ("assets", "original_sha256", "TEXT"),
    (
        "user_session",
        "auto_image_scaling",
        "INTEGER NOT NULL DEFAULT 1",
    ),
    ("user_session", "placement_gap", "REAL NOT NULL DEFAULT 0.0"),
    (
        "user_session",
        "import_target_canvas_area",
        "REAL NOT NULL DEFAULT 90000.0",
    ),
    (
        "user_session",
        "canvas_background",
        "TEXT NOT NULL DEFAULT 'none'",
    ),
    (
        "user_session",
        "semantic_search_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    ),
    (
        "user_session",
        "canvas_image_mode",
        "TEXT NOT NULL DEFAULT 'downscale'",
    ),
    (
        "user_session",
        "canvas_image_max_side",
        "INTEGER NOT NULL DEFAULT 2000",
    ),
    (
        "user_session",
        "rotation_snap_degrees",
        "REAL NOT NULL DEFAULT 15.0",
    ),
    (
        "user_session",
        "default_textbox_background",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    ("user_session", "sidebar_open", "INTEGER NOT NULL DEFAULT 0"),
    (
        "user_session",
        "sidebar_tab",
        "TEXT NOT NULL DEFAULT 'settings'",
    ),
    (
        "user_session",
        "active_board_id",
        "INTEGER NOT NULL DEFAULT 1",
    ),
    ("user_session", "session_payload", "TEXT"),
    ("asset_embeddings", "failure_kind", "TEXT"),
    (
        "asset_embeddings",
        "failure_count",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    ("asset_embeddings", "next_retry_at", "INTEGER"),
    (
        "canvas_annotations",
        "text_size",
        "TEXT NOT NULL DEFAULT 'm'",
    ),
    (
        "canvas_annotations",
        "stroke_width",
        "TEXT NOT NULL DEFAULT 'm'",
    ),
    (
        "canvas_annotations",
        "color",
        "TEXT NOT NULL DEFAULT 'default'",
    ),
    (
        "canvas_annotations",
        "background_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "canvas_annotations",
        "board_id",
        "INTEGER NOT NULL DEFAULT 1",
    ),
];

fn ensure_added_columns(connection: &Connection) -> Result<(), String> {
    for (table, column, definition) in ADDED_COLUMNS {
        if table_has_column(connection, table, column)? {
            continue;
        }
        connection
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                [],
            )
            .map_err(|error| format!("failed to add {column} column to {table}: {error}"))?;
    }
    Ok(())
}

fn ensure_board_items_asset_id_nullable(connection: &Connection) -> Result<(), String> {
    let asset_id_is_not_null = {
        let mut statement = connection
            .prepare("PRAGMA table_info(board_items)")
            .map_err(|error| format!("failed to inspect board_items schema: {error}"))?;
        let columns = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
            })
            .map_err(|error| format!("failed to read board_items schema: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board_items schema: {error}"))?;
        columns
            .iter()
            .any(|(name, not_null)| name == "asset_id" && *not_null != 0)
    };
    if !asset_id_is_not_null {
        return Ok(());
    }

    // Table rebuilds need foreign keys off outside the transaction; turn them
    // back on whether or not the rebuild succeeded.
    connection
        .execute_batch("PRAGMA foreign_keys = OFF")
        .map_err(|error| {
            format!("failed to disable foreign keys for board_items migration: {error}")
        })?;
    let migration = rebuild_board_items_with_nullable_asset_id(connection);
    let foreign_keys = connection
        .execute_batch("PRAGMA foreign_keys = ON")
        .map_err(|error| {
            format!("failed to re-enable foreign keys after board_items migration: {error}")
        });
    migration?;
    foreign_keys
}

fn rebuild_board_items_with_nullable_asset_id(connection: &Connection) -> Result<(), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| format!("failed to start board_items migration: {error}"))?;
    transaction
        .execute_batch(
            "
        ALTER TABLE board_items RENAME TO board_items_asset_required_old;

        CREATE TABLE board_items (
            id        INTEGER PRIMARY KEY,
            board_id  INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
            asset_id  INTEGER REFERENCES assets(id) ON DELETE CASCADE,
            x         REAL NOT NULL,
            y         REAL NOT NULL,
            scale     REAL NOT NULL,
            width     REAL NOT NULL DEFAULT 1.0,
            height    REAL NOT NULL DEFAULT 1.0,
            rotation  REAL NOT NULL,
            z_index   INTEGER NOT NULL,
            archived  INTEGER NOT NULL DEFAULT 0,
            placement_content_state TEXT NOT NULL DEFAULT 'loaded',
            placement_lifecycle TEXT NOT NULL DEFAULT 'committed',
            content_resolution_intent TEXT
        );

        INSERT INTO board_items (
            id, board_id, asset_id, x, y, scale, width, height, rotation, z_index, archived,
            placement_content_state, placement_lifecycle, content_resolution_intent
        )
        SELECT
            id, board_id, asset_id, x, y, scale, 1.0, 1.0, rotation, z_index, archived,
            placement_content_state, placement_lifecycle, content_resolution_intent
        FROM board_items_asset_required_old;

        DROP TABLE board_items_asset_required_old;
        ",
        )
        .map_err(|error| format!("failed to migrate board_items asset_id nullability: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("failed to commit board_items migration: {error}"))
}

fn ensure_asset_dedupe_index(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "CREATE INDEX IF NOT EXISTS idx_assets_exact_dedupe
             ON assets(archived, file_size, original_sha256, id)",
            [],
        )
        .map_err(|error| format!("failed to create asset dedupe index: {error}"))?;
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

/// Reads the settings columns that predate `session_payload`. Only the
/// one-time backfill below still needs them; the app reads the payload.
fn legacy_user_session_from_row(row: &rusqlite::Row<'_>) -> Result<UserSession, rusqlite::Error> {
    Ok(UserSession {
        settings: WorkspaceSettings {
            auto_image_scaling: row.get::<_, i64>(0)? != 0,
            placement_gap: row.get(1)?,
            placement_target_area: row
                .get::<_, f32>(2)
                .ok()
                .map(|value| value.max(1.0))
                .unwrap_or(crate::data::DEFAULT_PLACEMENT_TARGET_AREA),
            canvas_image_max_side: row
                .get::<_, i64>(4)
                .ok()
                .map(|value| value.max(1) as u32)
                .unwrap_or(crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE),
            semantic_search_first_launch_prompt_seen: false,
            rotation_snap_degrees: row
                .get::<_, f32>(5)
                .ok()
                .map(|value| value.max(0.0))
                .unwrap_or(crate::data::DEFAULT_ROTATION_SNAP_DEGREES),
            default_textbox_background: row.get::<_, i64>(6).unwrap_or(0) != 0,
            pointer_mode: crate::data::PointerMode::default(),
            always_on_top: false,
            last_update_check_at: None,
        },
        sidebar_open: row.get::<_, i64>(7).unwrap_or(0) != 0,
        sidebar_tab: row
            .get::<_, String>(8)
            .unwrap_or_else(|_| "settings".to_owned()),
        ..UserSession::default()
    })
}

fn backfill_user_session_payload(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT
                auto_image_scaling,
                placement_gap,
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

        let legacy = legacy_user_session_from_row(row)?;
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
                    archived      INTEGER NOT NULL DEFAULT 0,
                    original_sha256 TEXT
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
                    width     REAL NOT NULL DEFAULT 1.0,
                    height    REAL NOT NULL DEFAULT 1.0,
                    rotation  REAL NOT NULL,
                    z_index   INTEGER NOT NULL,
                    archived  INTEGER NOT NULL DEFAULT 0,
                    placement_content_state TEXT NOT NULL DEFAULT 'loaded',
                    placement_lifecycle TEXT NOT NULL DEFAULT 'committed',
                    content_resolution_intent TEXT
                );

                CREATE TABLE IF NOT EXISTS user_session (
                    id            INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x  REAL NOT NULL,
                    camera_pan_y  REAL NOT NULL,
                    camera_zoom   REAL NOT NULL,
                    auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                    placement_gap    REAL NOT NULL DEFAULT 0.0,
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

                CREATE TABLE IF NOT EXISTS document_identity (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    high_watermark INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS board_groups (
                    board_id INTEGER PRIMARY KEY REFERENCES boards(id),
                    payload TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS board_document_history (
                    id      INTEGER PRIMARY KEY CHECK (id = 1),
                    payload TEXT NOT NULL
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

        ensure_added_columns(&connection)?;
        ensure_asset_dedupe_index(&connection)?;
        ensure_default_board(&connection)?;
        ensure_board_items_asset_id_nullable(&connection)?;
        backfill_user_session_payload(&connection)?;
        ensure_canvas_stack_items_populated(&connection)?;
        if should_migrate_default_board_viewport {
            migrate_default_board_viewport(&connection)?;
        }
        Ok(())
    }
}
