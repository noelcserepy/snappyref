use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::{ColorType, ImageFormat};
use rusqlite::{Connection, Transaction, params};

use crate::data::{BoardItem, BoardItemWithAsset, ImageAsset, UserSession};

#[derive(Clone, Debug)]
pub struct Library {
    originals_dir: PathBuf,
    _thumbs_dir: PathBuf,
    db_path: PathBuf,
}

impl Library {
    pub fn initialize(root: PathBuf) -> Result<Self, String> {
        let originals_dir = root.join("originals");
        let thumbs_dir = root.join("thumbs");
        let db_path = root.join("library.db");

        fs::create_dir_all(&originals_dir)
            .map_err(|error| format!("failed to create originals dir: {error}"))?;
        fs::create_dir_all(&thumbs_dir)
            .map_err(|error| format!("failed to create thumbs dir: {error}"))?;

        let library = Self {
            originals_dir,
            _thumbs_dir: thumbs_dir,
            db_path,
        };
        library.initialize_schema()?;
        Ok(library)
    }

    pub fn store_original(&self, source_path: &Path) -> Result<PathBuf, String> {
        let original_name = source_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("image");
        let extension = source_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png");
        let destination = self.destination_path(original_name, extension);

        fs::copy(source_path, &destination)
            .map_err(|error| format!("failed to copy original: {error}"))?;
        Ok(destination)
    }

    pub fn store_clipboard_image(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        self.store_rgba_image("clipboard", rgba, width, height)
    }

    pub fn store_rgba_image(
        &self,
        source_name: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, "png");
        image::save_buffer_with_format(
            &destination,
            rgba,
            width,
            height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|error| format!("failed to write clipboard image: {error}"))?;
        Ok(destination)
    }

    pub fn persist_imported_image(
        &self,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        desired_world: egui::Pos2,
        initial_scale: f32,
    ) -> Result<BoardItemWithAsset, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start transaction: {error}"))?;

        let asset = insert_asset_row(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
        )?;
        let item = insert_board_item_row(&transaction, asset.id, desired_world, initial_scale)?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;

        Ok(BoardItemWithAsset { asset, item })
    }

    pub fn create_asset_and_relink_board_item(
        &self,
        item_id: i64,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        desired_world: egui::Pos2,
    ) -> Result<ImageAsset, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start relink transaction: {error}"))?;

        let asset = insert_asset_row(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
        )?;
        transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1, x = ?2, y = ?3
                 WHERE id = ?4",
                params![asset.id, desired_world.x, desired_world.y, item_id],
            )
            .map_err(|error| format!("failed to relink board item {item_id}: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit relink transaction: {error}"))?;

        Ok(asset)
    }

    pub fn restore_board_item_and_maybe_remove_asset(
        &self,
        item: &BoardItem,
        removed_asset_id: i64,
    ) -> Result<Option<PathBuf>, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start restore transaction: {error}"))?;

        transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1, x = ?2, y = ?3, scale = ?4, rotation = ?5, z_index = ?6, archived = ?7
                 WHERE id = ?8",
                params![
                    item.asset_id,
                    item.x,
                    item.y,
                    item.scale,
                    item.rotation,
                    item.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to restore board item {}: {error}", item.id))?;

        let removed_asset_path = transaction
            .query_row(
                "SELECT original_path
                 FROM assets
                 WHERE id = ?1",
                params![removed_asset_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .map(PathBuf::from);

        let asset_still_used: i64 = transaction
            .query_row(
                "SELECT COUNT(*)
                 FROM board_items
                 WHERE asset_id = ?1",
                params![removed_asset_id],
                |row| row.get(0),
            )
            .map_err(|error| {
                format!("failed to inspect asset references for {removed_asset_id}: {error}")
            })?;

        let removed_asset_path = if asset_still_used == 0 {
            transaction
                .execute(
                    "DELETE FROM assets WHERE id = ?1",
                    params![removed_asset_id],
                )
                .map_err(|error| format!("failed to delete asset {removed_asset_id}: {error}"))?;
            removed_asset_path
        } else {
            None
        };

        transaction
            .commit()
            .map_err(|error| format!("failed to commit restore transaction: {error}"))?;

        Ok(removed_asset_path)
    }

    pub fn load_board_items(&self) -> Result<Vec<BoardItemWithAsset>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    b.asset_id,
                    b.x,
                    b.y,
                    b.scale,
                    b.rotation,
                    b.z_index,
                    a.id,
                    a.original_path,
                    a.filename,
                    a.mime_type,
                    a.width,
                    a.height,
                    a.file_size,
                    a.imported_at
                 FROM board_items b
                 JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 0
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-items query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(BoardItemWithAsset {
                    item: BoardItem {
                        id: row.get(0)?,
                        asset_id: row.get(1)?,
                        x: row.get(2)?,
                        y: row.get(3)?,
                        scale: row.get(4)?,
                        rotation: row.get(5)?,
                        z_index: row.get(6)?,
                        archived: false,
                    },
                    asset: ImageAsset {
                        id: row.get(7)?,
                        original_path: PathBuf::from(row.get::<_, String>(8)?),
                        filename: row.get(9)?,
                        mime_type: row.get(10)?,
                        width: row.get::<_, u32>(11)?,
                        height: row.get::<_, u32>(12)?,
                        file_size: row.get::<_, i64>(13)? as u64,
                        imported_at: row.get(14)?,
                    },
                })
            })
            .map_err(|error| format!("failed to read board-items query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-items: {error}"))
    }

    pub fn update_board_item(&self, item: &BoardItem) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE board_items
                 SET x = ?1, y = ?2, scale = ?3, rotation = ?4, z_index = ?5, archived = ?6
                 WHERE id = ?7",
                params![
                    item.x,
                    item.y,
                    item.scale,
                    item.rotation,
                    item.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to update board item: {error}"))?;

        Ok(())
    }

    pub fn archive_board_items(&self, item_ids: &[i64]) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start archive transaction: {error}"))?;

        for item_id in item_ids {
            transaction
                .execute(
                    "UPDATE board_items SET archived = 1 WHERE id = ?1",
                    params![item_id],
                )
                .map_err(|error| format!("failed to archive board item {item_id}: {error}"))?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit archive transaction: {error}"))?;

        Ok(())
    }

    pub fn duplicate_board_items(
        &self,
        item_ids: &[i64],
        anchor_world: egui::Pos2,
        source_center_world: egui::Pos2,
    ) -> Result<Vec<BoardItemWithAsset>, String> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }

        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    b.asset_id,
                    b.x,
                    b.y,
                    b.scale,
                    b.rotation,
                    b.z_index,
                    a.id,
                    a.original_path,
                    a.filename,
                    a.mime_type,
                    a.width,
                    a.height,
                    a.file_size,
                    a.imported_at
                 FROM board_items b
                 JOIN assets a ON a.id = b.asset_id
                 WHERE b.id = ?1 AND b.archived = 0",
            )
            .map_err(|error| format!("failed to prepare duplicate-item query: {error}"))?;

        let mut source_records = Vec::new();
        for item_id in item_ids {
            let record = statement
                .query_row(params![item_id], |row| {
                    Ok(BoardItemWithAsset {
                        item: BoardItem {
                            id: row.get(0)?,
                            asset_id: row.get(1)?,
                            x: row.get(2)?,
                            y: row.get(3)?,
                            scale: row.get(4)?,
                            rotation: row.get(5)?,
                            z_index: row.get(6)?,
                            archived: false,
                        },
                        asset: ImageAsset {
                            id: row.get(7)?,
                            original_path: PathBuf::from(row.get::<_, String>(8)?),
                            filename: row.get(9)?,
                            mime_type: row.get(10)?,
                            width: row.get::<_, u32>(11)?,
                            height: row.get::<_, u32>(12)?,
                            file_size: row.get::<_, i64>(13)? as u64,
                            imported_at: row.get(14)?,
                        },
                    })
                })
                .map_err(|error| {
                    format!("failed to load board item {item_id} for duplication: {error}")
                })?;
            source_records.push(record);
        }

        let delta = anchor_world - source_center_world;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start duplicate transaction: {error}"))?;

        let mut duplicated = Vec::new();
        for source in source_records {
            transaction
                .execute(
                    "INSERT INTO board_items (
                        asset_id,
                        x,
                        y,
                        scale,
                        rotation,
                        z_index,
                        archived
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        source.item.asset_id,
                        source.item.x + delta.x,
                        source.item.y + delta.y,
                        source.item.scale,
                        source.item.rotation,
                        source.item.z_index,
                        0_i64
                    ],
                )
                .map_err(|error| {
                    format!("failed to duplicate board item {}: {error}", source.item.id)
                })?;

            let item_id = transaction.last_insert_rowid();
            duplicated.push(BoardItemWithAsset {
                asset: source.asset.clone(),
                item: BoardItem {
                    id: item_id,
                    asset_id: source.item.asset_id,
                    x: source.item.x + delta.x,
                    y: source.item.y + delta.y,
                    scale: source.item.scale,
                    rotation: source.item.rotation,
                    z_index: source.item.z_index,
                    archived: false,
                },
            });
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit duplicate transaction: {error}"))?;

        Ok(duplicated)
    }

    pub fn load_user_session(&self) -> Result<UserSession, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, import_gap
                 FROM user_session
                 WHERE id = 1",
            )
            .map_err(|error| format!("failed to prepare user-session query: {error}"))?;

        let session = statement.query_row([], |row| {
            Ok(UserSession {
                camera_pan_x: row.get(0)?,
                camera_pan_y: row.get(1)?,
                camera_zoom: row.get(2)?,
                auto_image_scaling: row.get::<_, i64>(3)? != 0,
                import_gap: row.get(4)?,
            })
        });

        match session {
            Ok(session) => Ok(session),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(UserSession::default()),
            Err(error) => Err(format!("failed to load user session: {error}")),
        }
    }

    pub fn save_user_session(&self, session: &UserSession) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, import_gap)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    camera_pan_x = excluded.camera_pan_x,
                    camera_pan_y = excluded.camera_pan_y,
                    camera_zoom = excluded.camera_zoom,
                    auto_image_scaling = excluded.auto_image_scaling,
                    import_gap = excluded.import_gap",
                params![
                    session.camera_pan_x,
                    session.camera_pan_y,
                    session.camera_zoom,
                    if session.auto_image_scaling { 1_i64 } else { 0_i64 },
                    session.import_gap,
                ],
            )
            .map_err(|error| format!("failed to save user session: {error}"))?;

        Ok(())
    }

    fn initialize_schema(&self) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute_batch(
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
                    imported_at   INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS board_items (
                    id        INTEGER PRIMARY KEY,
                    asset_id  INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    x         REAL NOT NULL,
                    y         REAL NOT NULL,
                    scale     REAL NOT NULL,
                    rotation  REAL NOT NULL,
                    z_index   INTEGER NOT NULL,
                    archived  INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS user_session (
                    id            INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x  REAL NOT NULL,
                    camera_pan_y  REAL NOT NULL,
                    camera_zoom   REAL NOT NULL,
                    auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                    import_gap    REAL NOT NULL DEFAULT 12.0
                );
                ",
            )
            .map_err(|error| format!("failed to initialize database schema: {error}"))?;

        ensure_board_items_archived_column(&connection)?;
        ensure_user_session_auto_image_scaling_column(&connection)?;
        ensure_user_session_import_gap_column(&connection)
    }

    fn connection(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path)
            .map_err(|error| format!("failed to open library database: {error}"))
    }

    fn destination_path(&self, source_name: &str, extension: &str) -> PathBuf {
        let timestamp = unix_timestamp_millis();
        let safe_name = sanitize_filename(source_name);
        self.originals_dir
            .join(format!("{timestamp}-{safe_name}.{extension}"))
    }
}

fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn sanitize_filename(input: &str) -> String {
    let sanitized: String = input
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
            _ => '-',
        })
        .collect();

    sanitized.trim_matches('-').to_owned().if_empty("image")
}

trait StringFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl StringFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_owned()
        } else {
            self
        }
    }
}

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

fn insert_asset_row(
    transaction: &Transaction<'_>,
    stored_path: &Path,
    mime_type: &str,
    width: u32,
    height: u32,
    file_size: u64,
) -> Result<ImageAsset, String> {
    let filename = stored_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("image.png")
        .to_owned();
    let mime_type = mime_type.to_owned();
    let imported_at = unix_timestamp_seconds();
    let original_path = stored_path.to_string_lossy().into_owned();

    transaction
        .execute(
            "INSERT INTO assets (
                original_path,
                filename,
                mime_type,
                width,
                height,
                file_size,
                imported_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                original_path,
                filename,
                mime_type,
                i64::from(width),
                i64::from(height),
                file_size as i64,
                imported_at,
            ],
        )
        .map_err(|error| format!("failed to insert asset: {error}"))?;

    Ok(ImageAsset {
        id: transaction.last_insert_rowid(),
        original_path: stored_path.to_path_buf(),
        filename,
        mime_type,
        width,
        height,
        file_size,
        imported_at,
    })
}

fn insert_board_item_row(
    transaction: &Transaction<'_>,
    asset_id: i64,
    desired_world: egui::Pos2,
    initial_scale: f32,
) -> Result<BoardItem, String> {
    transaction
        .execute(
            "INSERT INTO board_items (
                asset_id,
                x,
                y,
                scale,
                rotation,
                z_index,
                archived
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                asset_id,
                desired_world.x,
                desired_world.y,
                initial_scale,
                0.0_f32,
                0_i64,
                0_i64
            ],
        )
        .map_err(|error| format!("failed to insert board item: {error}"))?;

    Ok(BoardItem {
        id: transaction.last_insert_rowid(),
        asset_id,
        x: desired_world.x,
        y: desired_world.y,
        scale: initial_scale,
        rotation: 0.0,
        z_index: 0,
        archived: false,
    })
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
            "ALTER TABLE user_session ADD COLUMN import_gap REAL NOT NULL DEFAULT 12.0",
            [],
        )
        .map_err(|error| format!("failed to add import_gap column to user_session: {error}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Library;
    use rusqlite::Connection;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn existing_user_session_rows_migrate_to_default_preferences() {
        let root = std::env::temp_dir().join(format!(
            "reference-storage-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let db_path = root.join("library.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE user_session (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x REAL NOT NULL,
                    camera_pan_y REAL NOT NULL,
                    camera_zoom REAL NOT NULL
                );

                INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
                VALUES (1, 10.0, 20.0, 1.5);
                ",
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let session = library.load_user_session().unwrap();

        assert_eq!(session.camera_pan_x, 10.0);
        assert_eq!(session.camera_pan_y, 20.0);
        assert_eq!(session.camera_zoom, 1.5);
        assert!(session.auto_image_scaling);
        assert_eq!(session.import_gap, crate::data::DEFAULT_IMPORT_GAP);

        let _ = fs::remove_dir_all(root);
    }
}
