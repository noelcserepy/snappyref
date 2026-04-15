use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::{ColorType, ImageFormat};
use rusqlite::{Connection, params};

use crate::data::{BoardItem, BoardItemWithAsset, ImageAsset, UserSession};

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ArchivedSidebarEntry {
    pub item_id: i64,
    pub filename: String,
    pub imported_at: i64,
}

#[derive(Clone, Debug)]
pub struct Library {
    root: PathBuf,
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
            root,
            originals_dir,
            _thumbs_dir: thumbs_dir,
            db_path,
        };
        library.initialize_schema()?;
        Ok(library)
    }

    pub fn root(&self) -> &Path {
        &self.root
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
        let destination = self.destination_path("clipboard", "png");
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
    ) -> Result<BoardItemWithAsset, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start transaction: {error}"))?;

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
        let asset_id = transaction.last_insert_rowid();

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
                    1.0_f32,
                    0.0_f32,
                    0_i64,
                    0_i64
                ],
            )
            .map_err(|error| format!("failed to insert board item: {error}"))?;
        let item_id = transaction.last_insert_rowid();

        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;

        Ok(BoardItemWithAsset {
            asset: ImageAsset {
                id: asset_id,
                original_path: stored_path.to_path_buf(),
                filename,
                mime_type,
                width,
                height,
                file_size,
                imported_at,
            },
            item: BoardItem {
                id: item_id,
                asset_id,
                x: desired_world.x,
                y: desired_world.y,
                scale: 1.0,
                rotation: 0.0,
                z_index: 0,
                archived: false,
            },
        })
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

    pub fn load_archived_sidebar_entries(&self) -> Result<Vec<ArchivedSidebarEntry>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    a.filename,
                    a.imported_at
                 FROM board_items b
                 JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 1
                 ORDER BY a.imported_at DESC, a.filename ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare archived-items query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(ArchivedSidebarEntry {
                    item_id: row.get(0)?,
                    filename: row.get(1)?,
                    imported_at: row.get(2)?,
                })
            })
            .map_err(|error| format!("failed to read archived-items query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect archived-items: {error}"))
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

    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn delete_board_items(&self, item_ids: &[i64]) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start delete transaction: {error}"))?;

        for item_id in item_ids {
            transaction
                .execute("DELETE FROM board_items WHERE id = ?1", params![item_id])
                .map_err(|error| format!("failed to delete board item {item_id}: {error}"))?;
        }

        transaction
            .execute(
                "DELETE FROM assets
                 WHERE id NOT IN (SELECT asset_id FROM board_items)",
                [],
            )
            .map_err(|error| format!("failed to prune orphaned assets: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit delete transaction: {error}"))?;

        Ok(())
    }

    pub fn load_user_session(&self) -> Result<UserSession, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT camera_pan_x, camera_pan_y, camera_zoom
                 FROM user_session
                 WHERE id = 1",
            )
            .map_err(|error| format!("failed to prepare user-session query: {error}"))?;

        let session = statement.query_row([], |row| {
            Ok(UserSession {
                camera_pan_x: row.get(0)?,
                camera_pan_y: row.get(1)?,
                camera_zoom: row.get(2)?,
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
                "INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                    camera_pan_x = excluded.camera_pan_x,
                    camera_pan_y = excluded.camera_pan_y,
                    camera_zoom = excluded.camera_zoom",
                params![
                    session.camera_pan_x,
                    session.camera_pan_y,
                    session.camera_zoom,
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
                    camera_zoom   REAL NOT NULL
                );
                ",
            )
            .map_err(|error| format!("failed to initialize database schema: {error}"))?;

        ensure_board_items_archived_column(&connection)
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
