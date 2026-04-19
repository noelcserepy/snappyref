use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::{ColorType, ImageFormat};
use rusqlite::{Connection, Transaction, params, params_from_iter};

use crate::data::{
    BoardItem, BoardItemWithAsset, BoardLayoutItem, CanvasAnnotation, CanvasAnnotationKind,
    CanvasBackground, CanvasImageMode, ImageAsset, UserSession,
};

#[derive(Clone, Debug)]
pub struct Library {
    originals_dir: PathBuf,
    thumbs_dir: PathBuf,
    db_path: PathBuf,
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingStatus {
    Pending,
    Ready,
    Failed,
}

impl EmbeddingStatus {
    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct ImageEmbeddingJob {
    pub asset_id: i64,
    pub original_path: PathBuf,
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct ActiveCanvasEmbedding {
    pub asset_id: i64,
    pub embedding_blob: Vec<u8>,
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct AssetSearchHit {
    pub asset_id: i64,
    pub similarity: f32,
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct BoardSearchInstance {
    pub item_id: i64,
    pub asset_id: i64,
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
            thumbs_dir,
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn asset_thumb_path(&self, asset_id: i64) -> PathBuf {
        self.thumbs_dir.join(format!("asset-{asset_id}.png"))
    }

    pub fn canvas_image_cache_path(
        &self,
        asset_id: i64,
        mode: CanvasImageMode,
        max_side: u32,
    ) -> Option<PathBuf> {
        match mode {
            CanvasImageMode::Downscale => Some(self.thumbs_dir.join(format!(
                "asset-{asset_id}-canvas-v1-downscale-{max_side}.png"
            ))),
            CanvasImageMode::Original => None,
        }
    }

    pub fn store_canvas_image_cache(
        &self,
        asset_id: i64,
        mode: CanvasImageMode,
        max_side: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<PathBuf>, String> {
        let Some(destination) = self.canvas_image_cache_path(asset_id, mode, max_side) else {
            return Ok(None);
        };
        image::save_buffer_with_format(
            &destination,
            rgba,
            width,
            height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|error| format!("failed to write asset preview: {error}"))?;
        Ok(Some(destination))
    }

    pub fn remove_asset_canvas_caches(&self, asset_id: i64) -> Result<(), String> {
        let prefix = format!("asset-{asset_id}-");
        let entries = fs::read_dir(&self.thumbs_dir)
            .map_err(|error| format!("failed to read thumbs dir: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("failed to read thumbs entry: {error}"))?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if !file_name.starts_with(&prefix) {
                continue;
            }
            match fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove asset preview: {error}")),
            }
        }
        Ok(())
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

    pub fn load_board_layout_items(&self) -> Result<Vec<BoardLayoutItem>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    b.x,
                    b.y,
                    a.width * b.scale,
                    a.height * b.scale
                 FROM board_items b
                 JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 0
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-layout query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                Ok(BoardLayoutItem {
                    id: row.get(0)?,
                    x: row.get(1)?,
                    y: row.get(2)?,
                    width: row.get(3)?,
                    height: row.get(4)?,
                })
            })
            .map_err(|error| format!("failed to read board-layout query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-layout items: {error}"))
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

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn upsert_asset_embedding_status(
        &self,
        asset_id: i64,
        model_key: &str,
        dimension: usize,
        status: EmbeddingStatus,
        last_error: Option<&str>,
    ) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO asset_embeddings (
                    asset_id,
                    model_key,
                    dimension,
                    status,
                    last_error,
                    updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(asset_id, model_key) DO UPDATE SET
                    dimension = excluded.dimension,
                    status = excluded.status,
                    last_error = excluded.last_error,
                    updated_at = excluded.updated_at",
                params![
                    asset_id,
                    model_key,
                    dimension as i64,
                    status.as_str(),
                    last_error,
                    unix_timestamp_seconds(),
                ],
            )
            .map_err(|error| {
                format!("failed to upsert embedding status for asset {asset_id}: {error}")
            })?;
        Ok(())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn upsert_asset_embedding_blob(
        &self,
        asset_id: i64,
        model_key: &str,
        embedding: &[f32],
    ) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO asset_embeddings (
                    asset_id,
                    model_key,
                    dimension,
                    status,
                    last_error,
                    embedding_blob,
                    updated_at
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6)
                 ON CONFLICT(asset_id, model_key) DO UPDATE SET
                    dimension = excluded.dimension,
                    status = excluded.status,
                    last_error = NULL,
                    embedding_blob = excluded.embedding_blob,
                    updated_at = excluded.updated_at",
                params![
                    asset_id,
                    model_key,
                    embedding.len() as i64,
                    EmbeddingStatus::Ready.as_str(),
                    encode_f32_slice(embedding),
                    unix_timestamp_seconds(),
                ],
            )
            .map_err(|error| {
                format!("failed to upsert embedding blob for asset {asset_id}: {error}")
            })?;
        Ok(())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn missing_live_canvas_asset_jobs_for_model(
        &self,
        model_key: &str,
    ) -> Result<Vec<ImageEmbeddingJob>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    a.id,
                    a.original_path
                 FROM assets a
                 JOIN board_items b ON b.asset_id = a.id
                 LEFT JOIN asset_embeddings e
                    ON e.asset_id = a.id
                    AND e.model_key = ?1
                    AND e.status = 'ready'
                 WHERE b.archived = 0
                   AND e.asset_id IS NULL
                 GROUP BY a.id, a.original_path
                 ORDER BY a.id ASC",
            )
            .map_err(|error| format!("failed to prepare embedding-backfill query: {error}"))?;

        let rows = statement
            .query_map(params![model_key], |row| {
                Ok(ImageEmbeddingJob {
                    asset_id: row.get(0)?,
                    original_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .map_err(|error| format!("failed to read embedding-backfill query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect embedding backfill jobs: {error}"))
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn load_active_canvas_embeddings(
        &self,
        model_key: &str,
    ) -> Result<Vec<ActiveCanvasEmbedding>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    e.asset_id,
                    e.embedding_blob
                 FROM asset_embeddings e
                 JOIN board_items b ON b.asset_id = e.asset_id
                 WHERE b.archived = 0
                   AND e.model_key = ?1
                   AND e.status = 'ready'
                   AND e.embedding_blob IS NOT NULL
                 GROUP BY e.asset_id, e.embedding_blob
                 ORDER BY e.asset_id ASC",
            )
            .map_err(|error| format!("failed to prepare active-embedding query: {error}"))?;

        let rows = statement
            .query_map(params![model_key], |row| {
                Ok(ActiveCanvasEmbedding {
                    asset_id: row.get(0)?,
                    embedding_blob: row.get(1)?,
                })
            })
            .map_err(|error| format!("failed to read active-embedding query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect active embeddings: {error}"))
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn board_item_instances_for_assets(
        &self,
        asset_ids: &[i64],
    ) -> Result<Vec<BoardSearchInstance>, String> {
        if asset_ids.is_empty() {
            return Ok(Vec::new());
        }

        let connection = self.connection()?;
        let placeholders = std::iter::repeat_n("?", asset_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT
                id,
                asset_id
             FROM board_items
             WHERE archived = 0
               AND asset_id IN ({placeholders})
             ORDER BY z_index ASC, id ASC"
        );

        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("failed to prepare board-search-instance query: {error}"))?;

        let rows = statement
            .query_map(params_from_iter(asset_ids.iter().copied()), |row| {
                Ok(BoardSearchInstance {
                    item_id: row.get(0)?,
                    asset_id: row.get(1)?,
                })
            })
            .map_err(|error| format!("failed to read board-search-instance query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-search instances: {error}"))
    }

    pub fn load_user_session(&self) -> Result<UserSession, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    auto_image_scaling,
                    import_gap,
                    canvas_background,
                    semantic_search_enabled,
                    canvas_image_mode,
                    canvas_image_max_side
                 FROM user_session
                 WHERE id = 1",
            )
            .map_err(|error| format!("failed to prepare user-session query: {error}"))?;

        let session = statement.query_row([], |row| {
            let background = row
                .get::<_, String>(5)
                .ok()
                .and_then(|value| CanvasBackground::from_str(&value))
                .unwrap_or_default();
            let canvas_image_mode = row
                .get::<_, String>(7)
                .ok()
                .and_then(|value| CanvasImageMode::from_str(&value))
                .unwrap_or_default();
            Ok(UserSession {
                camera_pan_x: row.get(0)?,
                camera_pan_y: row.get(1)?,
                camera_zoom: row.get(2)?,
                auto_image_scaling: row.get::<_, i64>(3)? != 0,
                import_gap: row.get(4)?,
                canvas_image_mode,
                canvas_image_max_side: row
                    .get::<_, i64>(8)
                    .ok()
                    .map(|value| value.max(1) as u32)
                    .unwrap_or(crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE),
                canvas_background: background,
                semantic_search_enabled: row.get::<_, i64>(6).unwrap_or(0) != 0,
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
                "INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, import_gap, canvas_background, semantic_search_enabled, canvas_image_mode, canvas_image_max_side)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    camera_pan_x = excluded.camera_pan_x,
                    camera_pan_y = excluded.camera_pan_y,
                    camera_zoom = excluded.camera_zoom,
                    auto_image_scaling = excluded.auto_image_scaling,
                    import_gap = excluded.import_gap,
                    canvas_background = excluded.canvas_background,
                    semantic_search_enabled = excluded.semantic_search_enabled,
                    canvas_image_mode = excluded.canvas_image_mode,
                    canvas_image_max_side = excluded.canvas_image_max_side",
                params![
                    session.camera_pan_x,
                    session.camera_pan_y,
                    session.camera_zoom,
                    if session.auto_image_scaling { 1_i64 } else { 0_i64 },
                    session.import_gap,
                    session.canvas_background.as_str(),
                    if session.semantic_search_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    session.canvas_image_mode.as_str(),
                    i64::from(session.canvas_image_max_side),
                ],
            )
            .map_err(|error| format!("failed to save user session: {error}"))?;

        Ok(())
    }

    pub fn load_canvas_annotations(&self) -> Result<Vec<CanvasAnnotation>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, kind, start_x, start_y, end_x, end_y, text
                 FROM canvas_annotations
                 WHERE archived = 0
                 ORDER BY id ASC",
            )
            .map_err(|error| format!("failed to prepare canvas-annotations query: {error}"))?;

        let rows = statement
            .query_map([], |row| {
                let kind = row.get::<_, String>(1)?;
                Ok(CanvasAnnotation {
                    id: row.get(0)?,
                    kind: CanvasAnnotationKind::from_str(&kind).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("unknown canvas annotation kind: {kind}"),
                            )),
                        )
                    })?,
                    start_x: row.get(2)?,
                    start_y: row.get(3)?,
                    end_x: row.get(4)?,
                    end_y: row.get(5)?,
                    text: row.get(6)?,
                    archived: false,
                })
            })
            .map_err(|error| format!("failed to read canvas-annotations query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect canvas annotations: {error}"))
    }

    pub fn create_canvas_annotation(
        &self,
        kind: CanvasAnnotationKind,
        start: egui::Pos2,
        end: egui::Pos2,
        text: &str,
    ) -> Result<CanvasAnnotation, String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO canvas_annotations (
                    kind,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    text,
                    archived
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![kind.as_str(), start.x, start.y, end.x, end.y, text, 0_i64],
            )
            .map_err(|error| format!("failed to create canvas annotation: {error}"))?;

        Ok(CanvasAnnotation {
            id: connection.last_insert_rowid(),
            kind,
            start_x: start.x,
            start_y: start.y,
            end_x: end.x,
            end_y: end.y,
            text: text.to_owned(),
            archived: false,
        })
    }

    pub fn update_canvas_annotation(&self, annotation: &CanvasAnnotation) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE canvas_annotations
                 SET start_x = ?1,
                     start_y = ?2,
                     end_x = ?3,
                     end_y = ?4,
                     text = ?5,
                     archived = ?6
                 WHERE id = ?7",
                params![
                    annotation.start_x,
                    annotation.start_y,
                    annotation.end_x,
                    annotation.end_y,
                    annotation.text,
                    if annotation.archived { 1_i64 } else { 0_i64 },
                    annotation.id,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to update canvas annotation {}: {error}",
                    annotation.id
                )
            })?;

        Ok(())
    }

    pub fn archive_canvas_annotations(&self, annotation_ids: &[i64]) -> Result<(), String> {
        if annotation_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start annotation archive transaction: {error}"))?;

        for annotation_id in annotation_ids {
            transaction
                .execute(
                    "UPDATE canvas_annotations SET archived = 1 WHERE id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!("failed to archive canvas annotation {annotation_id}: {error}")
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation archive transaction: {error}"))?;

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
                    import_gap    REAL NOT NULL DEFAULT 12.0,
                    canvas_background TEXT NOT NULL DEFAULT 'dots',
                    semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                    canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                    canvas_image_max_side INTEGER NOT NULL DEFAULT 2000
                );

                CREATE TABLE IF NOT EXISTS canvas_annotations (
                    id        INTEGER PRIMARY KEY,
                    kind      TEXT NOT NULL,
                    start_x   REAL NOT NULL,
                    start_y   REAL NOT NULL,
                    end_x     REAL NOT NULL,
                    end_y     REAL NOT NULL,
                    text      TEXT NOT NULL DEFAULT '',
                    archived  INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS asset_embeddings (
                    asset_id        INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    model_key       TEXT NOT NULL,
                    dimension       INTEGER NOT NULL,
                    status          TEXT NOT NULL,
                    last_error      TEXT,
                    embedding_blob  BLOB,
                    updated_at      INTEGER NOT NULL,
                    PRIMARY KEY (asset_id, model_key),
                    CHECK (status IN ('pending', 'ready', 'failed'))
                );

                CREATE INDEX IF NOT EXISTS idx_asset_embeddings_model_status
                ON asset_embeddings(model_key, status);
                ",
            )
            .map_err(|error| format!("failed to initialize database schema: {error}"))?;

        ensure_board_items_archived_column(&connection)?;
        ensure_user_session_auto_image_scaling_column(&connection)?;
        ensure_user_session_import_gap_column(&connection)?;
        ensure_user_session_canvas_background_column(&connection)?;
        ensure_user_session_semantic_search_enabled_column(&connection)?;
        ensure_user_session_canvas_image_mode_column(&connection)?;
        ensure_user_session_canvas_image_max_side_column(&connection)
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
            "ALTER TABLE user_session ADD COLUMN canvas_background TEXT NOT NULL DEFAULT 'dots'",
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

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
fn encode_f32_slice(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::Library;
    use crate::data::{CanvasAnnotationKind, CanvasBackground, CanvasImageMode};
    use eframe::egui;
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
        assert_eq!(session.canvas_image_mode, CanvasImageMode::Downscale);
        assert_eq!(
            session.canvas_image_max_side,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
        );
        assert_eq!(session.canvas_background, CanvasBackground::Dots);
        assert!(session.semantic_search_enabled);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canvas_image_cache_path_is_deterministic() {
        let root = std::env::temp_dir().join(format!(
            "reference-thumb-path-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .canvas_image_cache_path(42, CanvasImageMode::Downscale, 2000)
            .unwrap();

        assert_eq!(
            path,
            root.join("thumbs")
                .join("asset-42-canvas-v1-downscale-2000.png")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn store_and_remove_canvas_image_cache_round_trips_thumb_file() {
        let root = std::env::temp_dir().join(format!(
            "reference-thumb-store-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let preview_path = library
            .store_canvas_image_cache(7, CanvasImageMode::Downscale, 2000, &[255, 0, 0, 255], 1, 1)
            .unwrap()
            .unwrap();

        assert!(preview_path.is_file());

        library.remove_asset_canvas_caches(7).unwrap();
        assert!(!preview_path.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remove_asset_canvas_caches_removes_all_variants_for_asset() {
        let root = std::env::temp_dir().join(format!(
            "reference-thumb-remove-variants-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let first = library
            .store_canvas_image_cache(7, CanvasImageMode::Downscale, 2000, &[255, 0, 0, 255], 1, 1)
            .unwrap()
            .unwrap();
        let second = library
            .store_canvas_image_cache(7, CanvasImageMode::Downscale, 4000, &[255, 0, 0, 255], 1, 1)
            .unwrap()
            .unwrap();

        library.remove_asset_canvas_caches(7).unwrap();

        assert!(!first.exists());
        assert!(!second.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn live_canvas_embedding_backfill_skips_ready_assets_and_archived_items() {
        let root = std::env::temp_dir().join(format!(
            "reference-embedding-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let ready_path = library
            .store_rgba_image("ready", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let pending_path = library
            .store_rgba_image("pending", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let archived_path = library
            .store_rgba_image("archived", &[0, 0, 255, 255], 1, 1)
            .unwrap();

        let ready = library
            .persist_imported_image(&ready_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let pending = library
            .persist_imported_image(
                &pending_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        let archived = library
            .persist_imported_image(
                &archived_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(20.0, 0.0),
                1.0,
            )
            .unwrap();

        library.archive_board_items(&[archived.item.id]).unwrap();
        library
            .upsert_asset_embedding_blob(ready.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();

        let jobs = library
            .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
            .unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].asset_id, pending.asset.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_helpers_scope_to_requested_model_key() {
        let root = std::env::temp_dir().join(format!(
            "reference-embedding-model-key-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let first_path = library
            .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let second_path = library
            .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let first = library
            .persist_imported_image(&first_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let second = library
            .persist_imported_image(
                &second_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();

        library
            .upsert_asset_embedding_blob(first.asset.id, "clip_key", &[1.0, 0.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(second.asset.id, "clip_key", &[0.0, 1.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(first.asset.id, "siglip_key", &[0.5, 0.5])
            .unwrap();

        let jobs = library
            .missing_live_canvas_asset_jobs_for_model("siglip_key")
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].asset_id, second.asset.id);

        let embeddings = library.load_active_canvas_embeddings("siglip_key").unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].asset_id, first.asset.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canvas_annotations_round_trip_and_archive() {
        let root = std::env::temp_dir().join(format!(
            "reference-annotation-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let created = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "hello",
            )
            .unwrap();

        let loaded = library.load_canvas_annotations().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], created);

        let mut updated = loaded[0].clone();
        updated.text = "updated".to_owned();
        library.update_canvas_annotation(&updated).unwrap();

        let loaded = library.load_canvas_annotations().unwrap();
        assert_eq!(loaded[0].text, "updated");

        library.archive_canvas_annotations(&[created.id]).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        let _ = fs::remove_dir_all(root);
    }
}
