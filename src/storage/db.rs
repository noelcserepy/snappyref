use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{ColorType, ExtendedColorType, ImageEncoder, ImageFormat};
use rusqlite::{Connection, Transaction, params, params_from_iter};

use crate::data::{
    Board, BoardItem, BoardItemWithAsset, BoardLayoutItem, CanvasAnnotation, CanvasAnnotationColor,
    CanvasAnnotationKind, CanvasBackground, CanvasImageLod, CanvasStackItem, CanvasStrokeWidth,
    CanvasTextSize, ImageAsset, UserSession, ViewportState, WorkspaceSettings,
};

#[derive(Clone, Debug)]
pub struct Library {
    originals_dir: PathBuf,
    thumbs_dir: PathBuf,
    db_path: PathBuf,
}

const USER_SESSION_PAYLOAD_VERSION: u32 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct StoredUserSessionPayload {
    #[serde(default = "current_user_session_payload_version")]
    version: u32,
    #[serde(default)]
    window: crate::data::WindowState,
    #[serde(default)]
    workspace_settings: WorkspaceSettings,
    #[serde(default)]
    sidebar: StoredSidebarState,
    #[serde(default)]
    recent_searches: Vec<String>,
}

impl StoredUserSessionPayload {
    fn from_session(session: &UserSession) -> Self {
        Self {
            version: USER_SESSION_PAYLOAD_VERSION,
            window: session.window.clone(),
            workspace_settings: session.settings.clone(),
            sidebar: StoredSidebarState {
                open: session.sidebar_open,
                tab: session.sidebar_tab.clone(),
            },
            recent_searches: session.recent_searches.clone(),
        }
    }
}

impl Default for StoredUserSessionPayload {
    fn default() -> Self {
        Self {
            version: USER_SESSION_PAYLOAD_VERSION,
            window: crate::data::WindowState::default(),
            workspace_settings: WorkspaceSettings::default(),
            sidebar: StoredSidebarState::default(),
            recent_searches: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct StoredSidebarState {
    #[serde(default)]
    open: bool,
    #[serde(default = "default_sidebar_tab")]
    tab: String,
}

impl Default for StoredSidebarState {
    fn default() -> Self {
        Self {
            open: false,
            tab: default_sidebar_tab(),
        }
    }
}

static DESTINATION_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingStatus {
    Pending,
    Ready,
    #[allow(dead_code)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CanvasImageCacheEncoding {
    Jpeg,
    WebpLossless,
}

impl CanvasImageCacheEncoding {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::WebpLossless => "webp",
        }
    }
}

fn choose_canvas_image_cache_encoding(rgba: &[u8]) -> CanvasImageCacheEncoding {
    if rgba.chunks_exact(4).all(|pixel| pixel[3] == u8::MAX) {
        CanvasImageCacheEncoding::Jpeg
    } else {
        CanvasImageCacheEncoding::WebpLossless
    }
}

fn canvas_image_cache_quality(lod: CanvasImageLod) -> u8 {
    match lod {
        CanvasImageLod::Overview => 72,
        CanvasImageLod::Working => 82,
        CanvasImageLod::Detail => 88,
    }
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    for pixel in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
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

    pub fn store_original_bytes(
        &self,
        source_name: &str,
        extension: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, extension);
        fs::write(&destination, bytes)
            .map_err(|error| format!("failed to write original bytes: {error}"))?;
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

    pub fn store_cropped_rgba_image(
        &self,
        source_name: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<PathBuf, String> {
        let destination = self.destination_path(source_name, "png");
        let output = File::create(&destination)
            .map_err(|error| format!("failed to create cropped image: {error}"))?;
        PngEncoder::new_with_quality(output, CompressionType::Fast, FilterType::NoFilter)
            .write_image(rgba, width, height, ExtendedColorType::Rgba8)
            .map_err(|error| format!("failed to write cropped image: {error}"))?;
        Ok(destination)
    }

    fn canvas_image_cache_destination_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        encoding: CanvasImageCacheEncoding,
    ) -> PathBuf {
        self.thumbs_dir.join(format!(
            "asset-{asset_id}-canvas-v3-downscale-{}-{max_side}.{}",
            lod.as_str(),
            encoding.extension()
        ))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn asset_thumb_path(&self, asset_id: i64) -> PathBuf {
        self.thumbs_dir.join(format!("asset-{asset_id}.png"))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn canvas_image_cache_path(&self, asset_id: i64, max_side: u32) -> Option<PathBuf> {
        self.canvas_image_cache_path_for_lod(asset_id, max_side, CanvasImageLod::Working)
    }

    pub fn canvas_image_cache_path_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
    ) -> Option<PathBuf> {
        [
            CanvasImageCacheEncoding::Jpeg,
            CanvasImageCacheEncoding::WebpLossless,
        ]
        .into_iter()
        .map(|encoding| {
            self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding)
        })
        .find(|path| path.is_file())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn asset_search_preview_paths_for_assets(
        &self,
        asset_ids: &[i64],
    ) -> std::collections::HashMap<i64, PathBuf> {
        asset_ids
            .iter()
            .filter_map(|asset_id| {
                self.canvas_image_cache_path_for_lod(
                    *asset_id,
                    crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                    CanvasImageLod::Overview,
                )
                .or_else(|| {
                    self.canvas_image_cache_path_for_lod(
                        *asset_id,
                        crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                        CanvasImageLod::Working,
                    )
                })
                .map(|path| (*asset_id, path))
            })
            .collect()
    }

    pub fn legacy_canvas_image_cache_path_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
    ) -> PathBuf {
        self.thumbs_dir.join(format!(
            "asset-{asset_id}-canvas-v2-downscale-{}-{max_side}.png",
            lod.as_str()
        ))
    }

    pub fn canvas_image_cache_destination_for_extension(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        extension: &str,
    ) -> Result<PathBuf, String> {
        let encoding = match extension {
            "jpg" => CanvasImageCacheEncoding::Jpeg,
            "webp" => CanvasImageCacheEncoding::WebpLossless,
            _ => {
                return Err(format!(
                    "unsupported canvas image cache extension {extension:?}"
                ));
            }
        };
        Ok(self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding))
    }

    pub fn store_canvas_image_cache(
        &self,
        asset_id: i64,
        max_side: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<PathBuf>, String> {
        self.store_canvas_image_cache_for_lod(
            asset_id,
            max_side,
            CanvasImageLod::Working,
            rgba,
            width,
            height,
        )
    }

    pub fn store_canvas_image_cache_for_lod(
        &self,
        asset_id: i64,
        max_side: u32,
        lod: CanvasImageLod,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Option<PathBuf>, String> {
        let encoding = choose_canvas_image_cache_encoding(rgba);
        let destination =
            self.canvas_image_cache_destination_for_lod(asset_id, max_side, lod, encoding);
        let file = File::create(&destination)
            .map_err(|error| format!("failed to create asset preview: {error}"))?;
        let writer = BufWriter::new(file);
        match encoding {
            CanvasImageCacheEncoding::Jpeg => {
                let rgb = rgba_to_rgb(rgba);
                let encoder =
                    JpegEncoder::new_with_quality(writer, canvas_image_cache_quality(lod));
                encoder
                    .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                    .map_err(|error| format!("failed to write asset preview: {error}"))?;
            }
            CanvasImageCacheEncoding::WebpLossless => {
                let encoder = WebPEncoder::new_lossless(writer);
                encoder
                    .write_image(rgba, width, height, ExtendedColorType::Rgba8)
                    .map_err(|error| format!("failed to write asset preview: {error}"))?;
            }
        }
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
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.persist_imported_image_to_board(
            active_board_id,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            desired_world,
            initial_scale,
        )
    }

    pub fn persist_imported_image_to_board(
        &self,
        board_id: i64,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        desired_world: egui::Pos2,
        initial_scale: f32,
    ) -> Result<BoardItemWithAsset, String> {
        let mut connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!(
                "cannot import to missing or archived board {board_id}"
            ));
        }
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
        let item = insert_board_item_row(
            &transaction,
            board_id,
            asset.id,
            desired_world,
            initial_scale,
        )?;
        append_canvas_stack_item(&transaction, board_id, CanvasStackItem::Placement(item.id))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;

        Ok(BoardItemWithAsset { asset, item })
    }

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
                    image_count: row.get::<_, i64>(3)? as usize,
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
                 ) VALUES (?1, 0, 0.0, 0.0, 1.0, 'none')",
                params![name],
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
                "DELETE FROM canvas_annotations
                 WHERE board_id = ?1",
                params![board_id],
            )
            .map_err(|error| format!("failed to delete board {board_id} annotations: {error}"))?;
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
                 VALUES (1, 0.0, 0.0, 1.0, ?1)
                 ON CONFLICT(id) DO UPDATE SET active_board_id = excluded.active_board_id",
                params![next_active_board_id],
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
                 VALUES (1, 0.0, 0.0, 1.0, ?1)
                 ON CONFLICT(id) DO UPDATE SET active_board_id = excluded.active_board_id",
                params![board_id],
            )
            .map_err(|error| format!("failed to persist active board: {error}"))?;

        Ok(())
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
        let active_board_id = active_board_id(&connection)?;
        self.load_board_items_for_board_id(&connection, active_board_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_board_items_for_board(
        &self,
        board_id: i64,
    ) -> Result<Vec<BoardItemWithAsset>, String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!("cannot load missing or archived board {board_id}"));
        }
        self.load_board_items_for_board_id(&connection, board_id)
    }

    fn load_board_items_for_board_id(
        &self,
        connection: &Connection,
        board_id: i64,
    ) -> Result<Vec<BoardItemWithAsset>, String> {
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
                   AND a.archived = 0
                   AND b.board_id = ?1
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-items query: {error}"))?;

        let rows = statement
            .query_map(params![board_id], |row| {
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
        let active_board_id = active_board_id(&connection)?;
        self.load_board_layout_items_for_board_id(&connection, active_board_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_board_layout_items_for_board(
        &self,
        board_id: i64,
    ) -> Result<Vec<BoardLayoutItem>, String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!("cannot load missing or archived board {board_id}"));
        }
        self.load_board_layout_items_for_board_id(&connection, board_id)
    }

    fn load_board_layout_items_for_board_id(
        &self,
        connection: &Connection,
        board_id: i64,
    ) -> Result<Vec<BoardLayoutItem>, String> {
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
                   AND a.archived = 0
                   AND b.board_id = ?1
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-layout query: {error}"))?;

        let rows = statement
            .query_map(params![board_id], |row| {
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

    pub fn load_asset(&self, asset_id: i64) -> Result<ImageAsset, String> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT
                    id,
                    original_path,
                    filename,
                    mime_type,
                    width,
                    height,
                    file_size,
                    imported_at
                 FROM assets
                 WHERE id = ?1
                   AND archived = 0",
                params![asset_id],
                |row| {
                    Ok(ImageAsset {
                        id: row.get(0)?,
                        original_path: PathBuf::from(row.get::<_, String>(1)?),
                        filename: row.get(2)?,
                        mime_type: row.get(3)?,
                        width: row.get::<_, u32>(4)?,
                        height: row.get::<_, u32>(5)?,
                        file_size: row.get::<_, i64>(6)? as u64,
                        imported_at: row.get(7)?,
                    })
                },
            )
            .map_err(|error| format!("failed to load asset {asset_id}: {error}"))
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
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'placement'
                       AND item_id = ?1",
                    params![item_id],
                )
                .map_err(|error| {
                    format!("failed to remove board item {item_id} from canvas stack: {error}")
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit archive transaction: {error}"))?;

        Ok(())
    }

    pub fn archive_assets(&self, asset_ids: &[i64]) -> Result<(), String> {
        if asset_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start asset archive transaction: {error}"))?;

        for asset_id in asset_ids {
            transaction
                .execute(
                    "UPDATE assets SET archived = 1 WHERE id = ?1",
                    params![asset_id],
                )
                .map_err(|error| format!("failed to archive asset {asset_id}: {error}"))?;
            transaction
                .execute(
                    "UPDATE board_items SET archived = 1 WHERE asset_id = ?1",
                    params![asset_id],
                )
                .map_err(|error| {
                    format!("failed to archive placements for asset {asset_id}: {error}")
                })?;
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'placement'
                       AND item_id IN (
                           SELECT id FROM board_items WHERE asset_id = ?1
                       )",
                    params![asset_id],
                )
                .map_err(|error| {
                    format!(
                        "failed to remove asset {asset_id} placements from canvas stack: {error}"
                    )
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit asset archive transaction: {error}"))?;

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
        let active_board_id = active_board_id(&connection)?;
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
                 WHERE b.id = ?1
                   AND b.archived = 0
                   AND b.board_id = ?2
                   AND a.archived = 0",
            )
            .map_err(|error| format!("failed to prepare duplicate-item query: {error}"))?;

        let mut source_records = Vec::new();
        for item_id in item_ids {
            let record = statement
                .query_row(params![item_id, active_board_id], |row| {
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
                        board_id,
                        asset_id,
                        x,
                        y,
                        scale,
                        rotation,
                        z_index,
                        archived
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        active_board_id,
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
            append_canvas_stack_item(
                &transaction,
                active_board_id,
                CanvasStackItem::Placement(item_id),
            )?;
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

    pub fn create_board_item_from_asset(
        &self,
        asset_id: i64,
        center_world: egui::Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
    ) -> Result<BoardItemWithAsset, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let asset = self.load_asset(asset_id)?;

        connection
            .execute(
                "INSERT INTO board_items (
                    board_id,
                    asset_id,
                    x,
                    y,
                    scale,
                    rotation,
                    z_index,
                    archived
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    active_board_id,
                    asset_id,
                    center_world.x,
                    center_world.y,
                    scale,
                    rotation,
                    z_index,
                    0_i64
                ],
            )
            .map_err(|error| format!("failed to create pasted board item: {error}"))?;

        let item_id = connection.last_insert_rowid();
        append_canvas_stack_item(
            &connection,
            active_board_id,
            CanvasStackItem::Placement(item_id),
        )?;

        Ok(BoardItemWithAsset {
            item: BoardItem {
                id: item_id,
                asset_id,
                x: center_world.x,
                y: center_world.y,
                scale,
                rotation,
                z_index,
                archived: false,
            },
            asset,
        })
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
    pub fn record_retryable_asset_embedding_failure(
        &self,
        asset_id: i64,
        model_key: &str,
        dimension: usize,
        last_error: &str,
        now: i64,
    ) -> Result<(), String> {
        let connection = self.connection()?;
        let previous_count = connection
            .query_row(
                "SELECT failure_count
                 FROM asset_embeddings
                 WHERE asset_id = ?1
                   AND model_key = ?2",
                params![asset_id, model_key],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        let failure_count = previous_count + 1;
        let (failure_kind, next_retry_at) = match failure_count {
            1 => ("retryable", Some(now)),
            2 => ("retryable", Some(now + 5 * 60)),
            _ => ("terminal", None),
        };

        connection
            .execute(
                "INSERT INTO asset_embeddings (
                    asset_id,
                    model_key,
                    dimension,
                    status,
                    last_error,
                    failure_kind,
                    failure_count,
                    next_retry_at,
                    updated_at
                 ) VALUES (?1, ?2, ?3, 'failed', ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(asset_id, model_key) DO UPDATE SET
                    dimension = excluded.dimension,
                    status = excluded.status,
                    last_error = excluded.last_error,
                    failure_kind = excluded.failure_kind,
                    failure_count = excluded.failure_count,
                    next_retry_at = excluded.next_retry_at,
                    updated_at = excluded.updated_at",
                params![
                    asset_id,
                    model_key,
                    dimension as i64,
                    last_error,
                    failure_kind,
                    failure_count,
                    next_retry_at,
                    now,
                ],
            )
            .map_err(|error| {
                format!("failed to record embedding failure for asset {asset_id}: {error}")
            })?;
        Ok(())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn record_terminal_asset_embedding_failure(
        &self,
        asset_id: i64,
        model_key: &str,
        dimension: usize,
        last_error: &str,
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
                    failure_kind,
                    failure_count,
                    next_retry_at,
                    updated_at
                 ) VALUES (?1, ?2, ?3, 'failed', ?4, 'terminal', 1, NULL, ?5)
                 ON CONFLICT(asset_id, model_key) DO UPDATE SET
                    dimension = excluded.dimension,
                    status = excluded.status,
                    last_error = excluded.last_error,
                    failure_kind = excluded.failure_kind,
                    failure_count = excluded.failure_count,
                    next_retry_at = excluded.next_retry_at,
                    updated_at = excluded.updated_at",
                params![
                    asset_id,
                    model_key,
                    dimension as i64,
                    last_error,
                    unix_timestamp_seconds(),
                ],
            )
            .map_err(|error| {
                format!("failed to record terminal embedding failure for asset {asset_id}: {error}")
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
                    failure_kind,
                    failure_count,
                    next_retry_at,
                    embedding_blob,
                    updated_at
                 ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, NULL, ?5, ?6)
                 ON CONFLICT(asset_id, model_key) DO UPDATE SET
                    dimension = excluded.dimension,
                    status = excluded.status,
                    last_error = NULL,
                    failure_kind = NULL,
                    failure_count = 0,
                    next_retry_at = NULL,
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
                 LEFT JOIN asset_embeddings e
                    ON e.asset_id = a.id
                    AND e.model_key = ?1
                 WHERE e.asset_id IS NULL
                   AND a.archived = 0
                   AND lower(trim(a.mime_type)) <> 'image/svg+xml'
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
    pub fn retryable_live_canvas_asset_jobs_for_model(
        &self,
        model_key: &str,
        now: i64,
    ) -> Result<Vec<ImageEmbeddingJob>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT
                    a.id,
                    a.original_path
                 FROM assets a
                 JOIN asset_embeddings e
                    ON e.asset_id = a.id
                    AND e.model_key = ?1
                 WHERE a.archived = 0
                   AND lower(trim(a.mime_type)) <> 'image/svg+xml'
                   AND e.status = 'failed'
                   AND e.failure_kind = 'retryable'
                   AND e.next_retry_at IS NOT NULL
                   AND e.next_retry_at <= ?2
                 GROUP BY a.id, a.original_path
                 ORDER BY e.next_retry_at ASC, a.id ASC",
            )
            .map_err(|error| {
                format!("failed to prepare embedding-retry backfill query: {error}")
            })?;

        let rows = statement
            .query_map(params![model_key, now], |row| {
                Ok(ImageEmbeddingJob {
                    asset_id: row.get(0)?,
                    original_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .map_err(|error| format!("failed to read embedding-retry backfill query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect embedding retry backfill jobs: {error}"))
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn missing_active_board_asset_jobs_for_model(
        &self,
        model_key: &str,
    ) -> Result<Vec<ImageEmbeddingJob>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT
                    a.id,
                    a.original_path
                 FROM assets a
                 JOIN board_items b
                    ON b.asset_id = a.id
                 LEFT JOIN asset_embeddings e
                    ON e.asset_id = a.id
                    AND e.model_key = ?1
                 WHERE e.asset_id IS NULL
                   AND a.archived = 0
                   AND lower(trim(a.mime_type)) <> 'image/svg+xml'
                   AND b.archived = 0
                   AND b.board_id = ?2
                 GROUP BY a.id, a.original_path
                 ORDER BY MIN(b.z_index) ASC, a.id ASC",
            )
            .map_err(|error| {
                format!("failed to prepare active-board embedding-backfill query: {error}")
            })?;

        let rows = statement
            .query_map(params![model_key, active_board_id], |row| {
                Ok(ImageEmbeddingJob {
                    asset_id: row.get(0)?,
                    original_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .map_err(|error| {
                format!("failed to read active-board embedding-backfill query: {error}")
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
            format!("failed to collect active-board embedding backfill jobs: {error}")
        })
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn retryable_active_board_asset_jobs_for_model(
        &self,
        model_key: &str,
        now: i64,
    ) -> Result<Vec<ImageEmbeddingJob>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT
                    a.id,
                    a.original_path
                 FROM assets a
                 JOIN board_items b
                    ON b.asset_id = a.id
                 JOIN asset_embeddings e
                    ON e.asset_id = a.id
                    AND e.model_key = ?1
                 WHERE a.archived = 0
                   AND lower(trim(a.mime_type)) <> 'image/svg+xml'
                   AND b.archived = 0
                   AND b.board_id = ?2
                   AND e.status = 'failed'
                   AND e.failure_kind = 'retryable'
                   AND e.next_retry_at IS NOT NULL
                   AND e.next_retry_at <= ?3
                 GROUP BY a.id, a.original_path
                 ORDER BY e.next_retry_at ASC, MIN(b.z_index) ASC, a.id ASC",
            )
            .map_err(|error| {
                format!("failed to prepare active-board embedding-retry query: {error}")
            })?;

        let rows = statement
            .query_map(params![model_key, active_board_id, now], |row| {
                Ok(ImageEmbeddingJob {
                    asset_id: row.get(0)?,
                    original_path: PathBuf::from(row.get::<_, String>(1)?),
                })
            })
            .map_err(|error| {
                format!("failed to read active-board embedding-retry query: {error}")
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|error| {
            format!("failed to collect active-board embedding retry jobs: {error}")
        })
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn next_embedding_backfill_job_for_model(
        &self,
        model_key: &str,
        now: i64,
    ) -> Result<Option<ImageEmbeddingJob>, String> {
        if let Some(job) = self
            .missing_active_board_asset_jobs_for_model(model_key)?
            .into_iter()
            .next()
        {
            return Ok(Some(job));
        }
        if let Some(job) = self
            .missing_live_canvas_asset_jobs_for_model(model_key)?
            .into_iter()
            .next()
        {
            return Ok(Some(job));
        }
        if let Some(job) = self
            .retryable_active_board_asset_jobs_for_model(model_key, now)?
            .into_iter()
            .next()
        {
            return Ok(Some(job));
        }
        self.retryable_live_canvas_asset_jobs_for_model(model_key, now)
            .map(|jobs| jobs.into_iter().next())
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn load_active_canvas_embeddings(
        &self,
        model_key: &str,
    ) -> Result<Vec<ActiveCanvasEmbedding>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT
                    e.asset_id,
                    e.embedding_blob
                 FROM asset_embeddings e
                 JOIN board_items b ON b.asset_id = e.asset_id
                 JOIN assets a ON a.id = e.asset_id
                 WHERE b.archived = 0
                   AND b.board_id = ?2
                   AND a.archived = 0
                   AND e.model_key = ?1
                   AND e.status = 'ready'
                   AND e.embedding_blob IS NOT NULL
                 GROUP BY e.asset_id, e.embedding_blob
                 ORDER BY e.asset_id ASC",
            )
            .map_err(|error| format!("failed to prepare active-embedding query: {error}"))?;

        let rows = statement
            .query_map(params![model_key, active_board_id], |row| {
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
    pub fn load_library_embeddings(
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
                 JOIN assets a ON a.id = e.asset_id
                 WHERE e.model_key = ?1
                   AND e.status = 'ready'
                   AND e.embedding_blob IS NOT NULL
                   AND a.archived = 0
                 ORDER BY e.asset_id ASC",
            )
            .map_err(|error| format!("failed to prepare library-embedding query: {error}"))?;

        let rows = statement
            .query_map(params![model_key], |row| {
                Ok(ActiveCanvasEmbedding {
                    asset_id: row.get(0)?,
                    embedding_blob: row.get(1)?,
                })
            })
            .map_err(|error| format!("failed to read library-embedding query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect library embeddings: {error}"))
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn load_asset_embedding(
        &self,
        asset_id: i64,
        model_key: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT embedding_blob
                 FROM asset_embeddings
                 JOIN assets a ON a.id = asset_embeddings.asset_id
                 WHERE asset_embeddings.asset_id = ?1
                   AND asset_embeddings.model_key = ?2
                   AND asset_embeddings.status = 'ready'
                   AND asset_embeddings.embedding_blob IS NOT NULL
                   AND a.archived = 0
                 LIMIT 1",
            )
            .map_err(|error| format!("failed to prepare asset-embedding query: {error}"))?;

        let mut rows = statement
            .query(params![asset_id, model_key])
            .map_err(|error| format!("failed to read asset-embedding query: {error}"))?;

        match rows
            .next()
            .map_err(|error| format!("failed to fetch asset embedding: {error}"))?
        {
            Some(row) => {
                let blob: Vec<u8> = row
                    .get(0)
                    .map_err(|error| format!("failed to decode asset embedding: {error}"))?;
                Ok(Some(blob))
            }
            None => Ok(None),
        }
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
        let active_board_id = active_board_id(&connection)?;
        let placeholders = std::iter::repeat_n("?", asset_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT
                board_items.id,
                board_items.asset_id
             FROM board_items
             JOIN assets ON assets.id = board_items.asset_id
             WHERE board_items.archived = 0
               AND board_items.board_id = ?
               AND assets.archived = 0
               AND board_items.asset_id IN ({placeholders})
             ORDER BY board_items.z_index ASC, board_items.id ASC"
        );

        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("failed to prepare board-search-instance query: {error}"))?;

        let rows = statement
            .query_map(
                params_from_iter(std::iter::once(active_board_id).chain(asset_ids.iter().copied())),
                |row| {
                    Ok(BoardSearchInstance {
                        item_id: row.get(0)?,
                        asset_id: row.get(1)?,
                    })
                },
            )
            .map_err(|error| format!("failed to read board-search-instance query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-search instances: {error}"))
    }

    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    pub fn asset_original_paths_for_assets(
        &self,
        asset_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, PathBuf>, String> {
        if asset_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let connection = self.connection()?;
        let placeholders = std::iter::repeat_n("?", asset_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, original_path
             FROM assets
             WHERE archived = 0
               AND id IN ({placeholders})"
        );

        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| format!("failed to prepare asset-path query: {error}"))?;

        let rows = statement
            .query_map(params_from_iter(asset_ids.iter().copied()), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                ))
            })
            .map_err(|error| format!("failed to read asset-path query: {error}"))?;

        rows.collect::<Result<std::collections::HashMap<_, _>, _>>()
            .map_err(|error| format!("failed to collect asset paths: {error}"))
    }

    pub fn load_user_session(&self) -> Result<UserSession, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let viewport = board_viewport_state(&connection, active_board_id)
            .map_err(|error| format!("failed to load active board viewport: {error}"))?;
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
            .map_err(|error| format!("failed to prepare user-session query: {error}"))?;

        let session = statement.query_row([], |row| {
            let legacy = legacy_user_session_from_row(row, active_board_id, viewport.clone())?;
            let payload = row.get::<_, Option<String>>(9).ok().flatten();
            Ok(apply_user_session_payload(legacy, payload))
        });

        match session {
            Ok(session) => Ok(session),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let mut session = UserSession::default();
                session.active_board_id = active_board_id;
                session.viewport = viewport;
                Ok(session)
            }
            Err(error) => Err(format!("failed to load user session: {error}")),
        }
    }

    pub fn load_session_viewport(&self) -> Result<(i64, ViewportState), String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let viewport = board_viewport_state(&connection, active_board_id)
            .map_err(|error| format!("failed to load active board viewport: {error}"))?;
        Ok((active_board_id, viewport))
    }

    pub fn save_user_session(&self, session: &UserSession) -> Result<(), String> {
        let connection = self.connection()?;
        let active_board_id = if board_is_non_archived(&connection, session.active_board_id)? {
            session.active_board_id
        } else {
            active_board_id(&connection)?
        };
        let session_payload = encode_user_session_payload(session)?;
        connection
            .execute(
                "INSERT INTO user_session (
                    id,
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    auto_image_scaling,
                    import_gap,
                    import_target_canvas_area,
                    canvas_background,
                    semantic_search_enabled,
                    canvas_image_mode,
                    canvas_image_max_side,
                    rotation_snap_degrees,
                    default_textbox_background,
                    sidebar_open,
                    sidebar_tab,
                    session_payload,
                    active_board_id
                 )
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                    camera_pan_x = excluded.camera_pan_x,
                    camera_pan_y = excluded.camera_pan_y,
                    camera_zoom = excluded.camera_zoom,
                    auto_image_scaling = excluded.auto_image_scaling,
                    import_gap = excluded.import_gap,
                    import_target_canvas_area = excluded.import_target_canvas_area,
                    canvas_background = excluded.canvas_background,
                    semantic_search_enabled = excluded.semantic_search_enabled,
                    canvas_image_mode = excluded.canvas_image_mode,
                    canvas_image_max_side = excluded.canvas_image_max_side,
                    rotation_snap_degrees = excluded.rotation_snap_degrees,
                    default_textbox_background = excluded.default_textbox_background,
                    sidebar_open = excluded.sidebar_open,
                    sidebar_tab = excluded.sidebar_tab,
                    session_payload = excluded.session_payload,
                    active_board_id = excluded.active_board_id",
                params![
                    session.viewport.camera_pan_x,
                    session.viewport.camera_pan_y,
                    session.viewport.camera_zoom,
                    if session.settings.auto_image_scaling {
                        1_i64
                    } else {
                        0_i64
                    },
                    session.settings.import_gap,
                    session.settings.import_target_canvas_area,
                    session.viewport.canvas_background.as_str(),
                    if session.settings.semantic_search_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    "downscale",
                    i64::from(session.settings.canvas_image_max_side),
                    session.settings.rotation_snap_degrees,
                    if session.settings.default_textbox_background {
                        1_i64
                    } else {
                        0_i64
                    },
                    if session.sidebar_open { 1_i64 } else { 0_i64 },
                    session.sidebar_tab.as_str(),
                    session_payload,
                    active_board_id,
                ],
            )
            .map_err(|error| format!("failed to save user session: {error}"))?;

        connection
            .execute(
                "UPDATE boards
                 SET camera_pan_x = ?1,
                     camera_pan_y = ?2,
                     camera_zoom = ?3,
                     canvas_background = ?4
                 WHERE id = ?5",
                params![
                    session.viewport.camera_pan_x,
                    session.viewport.camera_pan_y,
                    session.viewport.camera_zoom,
                    session.viewport.canvas_background.as_str(),
                    active_board_id,
                ],
            )
            .map_err(|error| format!("failed to save board viewport: {error}"))?;

        Ok(())
    }

    pub fn load_canvas_annotations(&self) -> Result<Vec<CanvasAnnotation>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT
                    id,
                    kind,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    text,
                    text_size,
                    stroke_width,
                    color,
                    background_enabled
                 FROM canvas_annotations
                 WHERE archived = 0
                   AND board_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(|error| format!("failed to prepare canvas-annotations query: {error}"))?;

        let rows = statement
            .query_map(params![active_board_id], |row| {
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
                    text_size: CanvasTextSize::from_str(&row.get::<_, String>(7)?).ok_or_else(
                        || {
                            rusqlite::Error::FromSqlConversionFailure(
                                7,
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "unknown canvas text size",
                                )),
                            )
                        },
                    )?,
                    stroke_width: CanvasStrokeWidth::from_str(&row.get::<_, String>(8)?)
                        .ok_or_else(|| {
                            rusqlite::Error::FromSqlConversionFailure(
                                8,
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "unknown canvas stroke width",
                                )),
                            )
                        })?,
                    color: CanvasAnnotationColor::from_str(&row.get::<_, String>(9)?).ok_or_else(
                        || {
                            rusqlite::Error::FromSqlConversionFailure(
                                9,
                                rusqlite::types::Type::Text,
                                Box::new(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "unknown canvas annotation color",
                                )),
                            )
                        },
                    )?,
                    background_enabled: row.get::<_, i64>(10).unwrap_or(0) != 0,
                    archived: false,
                })
            })
            .map_err(|error| format!("failed to read canvas-annotations query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect canvas annotations: {error}"))
    }

    pub fn load_canvas_stack_order(&self) -> Result<Vec<CanvasStackItem>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        load_canvas_stack_order_for_board_id(&connection, active_board_id)
    }

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

    pub fn create_canvas_annotation(
        &self,
        kind: CanvasAnnotationKind,
        start: egui::Pos2,
        end: egui::Pos2,
        text: &str,
        text_size: CanvasTextSize,
        stroke_width: CanvasStrokeWidth,
        color: CanvasAnnotationColor,
        background_enabled: bool,
    ) -> Result<CanvasAnnotation, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        connection
            .execute(
                "INSERT INTO canvas_annotations (
                    board_id,
                    kind,
                    start_x,
                    start_y,
                    end_x,
                    end_y,
                    text,
                    text_size,
                    stroke_width,
                    color,
                    background_enabled,
                    archived
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    active_board_id,
                    kind.as_str(),
                    start.x,
                    start.y,
                    end.x,
                    end.y,
                    text,
                    text_size.as_str(),
                    stroke_width.as_str(),
                    color.as_str(),
                    if background_enabled { 1_i64 } else { 0_i64 },
                    0_i64
                ],
            )
            .map_err(|error| format!("failed to create canvas annotation: {error}"))?;

        let annotation_id = connection.last_insert_rowid();
        append_canvas_stack_item(
            &connection,
            active_board_id,
            CanvasStackItem::Annotation(annotation_id),
        )?;

        Ok(CanvasAnnotation {
            id: annotation_id,
            kind,
            start_x: start.x,
            start_y: start.y,
            end_x: end.x,
            end_y: end.y,
            text: text.to_owned(),
            text_size,
            stroke_width,
            color,
            background_enabled,
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
                     text_size = ?6,
                     stroke_width = ?7,
                     color = ?8,
                     background_enabled = ?9,
                     archived = ?10
                 WHERE id = ?11",
                params![
                    annotation.start_x,
                    annotation.start_y,
                    annotation.end_x,
                    annotation.end_y,
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
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'annotation'
                       AND item_id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!(
                        "failed to remove canvas annotation {annotation_id} from stack: {error}"
                    )
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation archive transaction: {error}"))?;

        Ok(())
    }

    pub fn delete_canvas_annotations(&self, annotation_ids: &[i64]) -> Result<(), String> {
        if annotation_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start annotation delete transaction: {error}"))?;

        for annotation_id in annotation_ids {
            transaction
                .execute(
                    "DELETE FROM canvas_annotations WHERE id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!("failed to delete canvas annotation {annotation_id}: {error}")
                })?;
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'annotation'
                       AND item_id = ?1",
                    params![annotation_id],
                )
                .map_err(|error| {
                    format!(
                        "failed to remove deleted canvas annotation {annotation_id} from stack: {error}"
                    )
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit annotation delete transaction: {error}"))?;

        Ok(())
    }

    fn initialize_schema(&self) -> Result<(), String> {
        let connection = self.connection()?;
        let should_migrate_default_board_viewport = !table_exists(&connection, "boards")?;
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
                    imported_at   INTEGER NOT NULL,
                    archived      INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS boards (
                    id            INTEGER PRIMARY KEY,
                    name          TEXT NOT NULL,
                    archived      INTEGER NOT NULL DEFAULT 0,
                    camera_pan_x  REAL NOT NULL DEFAULT 0.0,
                    camera_pan_y  REAL NOT NULL DEFAULT 0.0,
                    camera_zoom   REAL NOT NULL DEFAULT 1.0,
                    canvas_background TEXT NOT NULL DEFAULT 'none'
                );

                CREATE TABLE IF NOT EXISTS board_items (
                    id        INTEGER PRIMARY KEY,
                    board_id  INTEGER NOT NULL DEFAULT 1 REFERENCES boards(id),
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
                    import_gap    REAL NOT NULL DEFAULT 24.0,
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
            )
            .map_err(|error| format!("failed to initialize database schema: {error}"))?;

        ensure_board_items_archived_column(&connection)?;
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

    fn connection(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path)
            .map_err(|error| format!("failed to open library database: {error}"))
    }

    fn destination_path(&self, source_name: &str, extension: &str) -> PathBuf {
        let timestamp = unix_timestamp_millis();
        let sequence = DESTINATION_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
        let safe_name = sanitize_filename(source_name);
        self.originals_dir
            .join(format!("{timestamp}-{sequence}-{safe_name}.{extension}"))
    }
}

fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn current_user_session_payload_version() -> u32 {
    USER_SESSION_PAYLOAD_VERSION
}

fn default_sidebar_tab() -> String {
    "settings".to_owned()
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
                 ) VALUES (1, 'My Board', 0, 0.0, 0.0, 1.0, 'none')",
                [],
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

fn load_canvas_stack_order_for_board_id(
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
                           JOIN assets ON assets.id = board_items.asset_id
                           WHERE board_items.id = canvas_stack_items.item_id
                             AND board_items.board_id = canvas_stack_items.board_id
                             AND board_items.archived = 0
                             AND assets.archived = 0
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

fn append_canvas_stack_item(
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

fn ensure_canvas_stack_items_populated(connection: &Connection) -> Result<(), String> {
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

fn active_board_id(connection: &Connection) -> Result<i64, String> {
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

fn list_non_archived_board_ids(connection: &Connection) -> Result<Vec<i64>, String> {
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

fn board_is_non_archived(connection: &Connection, board_id: i64) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM boards WHERE id = ?1 AND archived = 0",
            params![board_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("failed to inspect board {board_id}: {error}"))?;
    Ok(count > 0)
}

fn board_viewport_state(
    connection: &Connection,
    board_id: i64,
) -> Result<ViewportState, rusqlite::Error> {
    connection.query_row(
        "SELECT camera_pan_x, camera_pan_y, camera_zoom, canvas_background
         FROM boards
         WHERE id = ?1",
        params![board_id],
        |row| {
            let background = row
                .get::<_, String>(3)
                .ok()
                .and_then(|value| CanvasBackground::from_str(&value))
                .unwrap_or_default();
            Ok(ViewportState {
                camera_pan_x: row.get(0)?,
                camera_pan_y: row.get(1)?,
                camera_zoom: row.get(2)?,
                canvas_background: background,
            })
        },
    )
}

fn legacy_user_session_from_row(
    row: &rusqlite::Row<'_>,
    active_board_id: i64,
    viewport: ViewportState,
) -> Result<UserSession, rusqlite::Error> {
    Ok(UserSession {
        active_board_id,
        viewport,
        window: crate::data::WindowState::default(),
        settings: WorkspaceSettings {
            auto_image_scaling: row.get::<_, i64>(0)? != 0,
            import_gap: row.get(1)?,
            import_target_canvas_area: row
                .get::<_, f32>(2)
                .ok()
                .map(|value| value.max(1.0))
                .unwrap_or(crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA),
            canvas_image_max_side: row
                .get::<_, i64>(4)
                .ok()
                .map(|value| value.max(1) as u32)
                .unwrap_or(crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE),
            semantic_search_enabled: row.get::<_, i64>(3).unwrap_or(0) != 0,
            semantic_search_first_launch_prompt_seen: false,
            rotation_snap_degrees: row
                .get::<_, f32>(5)
                .ok()
                .map(|value| value.max(0.0))
                .unwrap_or(crate::data::DEFAULT_ROTATION_SNAP_DEGREES),
            default_textbox_background: row.get::<_, i64>(6).unwrap_or(0) != 0,
            pointer_mode: crate::data::PointerMode::default(),
            last_update_check_at: None,
        },
        sidebar_open: row.get::<_, i64>(7).unwrap_or(0) != 0,
        sidebar_tab: row
            .get::<_, String>(8)
            .unwrap_or_else(|_| default_sidebar_tab()),
        recent_searches: Vec::new(),
    })
}

fn apply_user_session_payload(mut session: UserSession, payload: Option<String>) -> UserSession {
    let Some(payload) = payload else {
        return session;
    };
    if payload.trim().is_empty() {
        return session;
    }

    let Ok(stored) = serde_json::from_str::<StoredUserSessionPayload>(&payload) else {
        return session;
    };

    session.settings = stored.workspace_settings;
    session.window = stored.window;
    session.sidebar_open = stored.sidebar.open;
    session.sidebar_tab = stored.sidebar.tab;
    session.recent_searches = stored.recent_searches;
    session
}

fn encode_user_session_payload(session: &UserSession) -> Result<String, String> {
    serde_json::to_string(&StoredUserSessionPayload::from_session(session))
        .map_err(|error| format!("failed to encode user session payload: {error}"))
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
    board_id: i64,
    asset_id: i64,
    desired_world: egui::Pos2,
    initial_scale: f32,
) -> Result<BoardItem, String> {
    transaction
        .execute(
            "INSERT INTO board_items (
                board_id,
                asset_id,
                x,
                y,
                scale,
                rotation,
                z_index,
                archived
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                board_id,
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
            "ALTER TABLE user_session ADD COLUMN import_gap REAL NOT NULL DEFAULT 24.0",
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
    use super::{EmbeddingStatus, Library};
    use crate::data::{
        CanvasAnnotationKind, CanvasBackground, CanvasStackItem, CanvasTextSize, UserSession,
    };
    use eframe::egui;
    use rusqlite::Connection;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn update_preservation_smoke() {
        let app_root = temp_root("snappyref-update-preservation-test");
        let library_root = app_root.join("library");
        fs::create_dir_all(&library_root).unwrap();
        fs::write(app_root.join("SnappyRef.app"), b"old app bundle").unwrap();
        fs::write(app_root.join("UPDATE.txt"), b"old update notes").unwrap();

        let original_bytes = [255, 0, 0, 255];
        let library = Library::initialize(library_root.clone()).unwrap();
        let first_board = library.list_boards().unwrap()[0].clone();
        let second_board = library.create_board("Second").unwrap();
        library.set_active_board(second_board.id).unwrap();
        let original_path = library
            .store_rgba_image("preserved-original", &original_bytes, 1, 1)
            .unwrap();
        let imported = library
            .persist_imported_image(
                &original_path,
                "image/png",
                1,
                1,
                original_bytes.len() as u64,
                egui::pos2(42.0, 24.0),
                1.5,
            )
            .unwrap();
        let mut session = UserSession {
            active_board_id: second_board.id,
            ..UserSession::default()
        };
        session.viewport.camera_pan_x = 123.0;
        session.viewport.camera_pan_y = 456.0;
        session.viewport.camera_zoom = 1.75;
        session.viewport.canvas_background = CanvasBackground::Grid;
        session.settings.auto_image_scaling = false;
        session.settings.import_gap = 36.0;
        session.settings.import_target_canvas_area = 54_321.0;
        session.settings.semantic_search_enabled = false;
        session.settings.canvas_image_max_side = 4096;
        session.settings.rotation_snap_degrees = 30.0;
        session.settings.default_textbox_background = true;
        session.sidebar_open = true;
        session.sidebar_tab = "boards".to_owned();
        library.save_user_session(&session).unwrap();
        let stored_original_bytes = fs::read(&imported.asset.original_path).unwrap();
        drop(library);

        fs::remove_file(app_root.join("SnappyRef.app")).unwrap();
        fs::remove_file(app_root.join("UPDATE.txt")).unwrap();
        fs::write(app_root.join("SnappyRef.app"), b"new app bundle").unwrap();
        fs::write(app_root.join("UPDATE.txt"), b"new update notes").unwrap();

        let reopened = Library::initialize(library_root.clone()).unwrap();
        let boards = reopened.list_boards().unwrap();
        let reloaded_session = reopened.load_user_session().unwrap();
        let items = reopened.load_board_items().unwrap();

        assert_eq!(
            boards.iter().map(|board| board.id).collect::<Vec<_>>(),
            vec![first_board.id, second_board.id]
        );
        assert_eq!(reloaded_session.active_board_id, second_board.id);
        assert_eq!(reloaded_session.viewport.camera_pan_x, 123.0);
        assert_eq!(reloaded_session.viewport.camera_pan_y, 456.0);
        assert_eq!(reloaded_session.viewport.camera_zoom, 1.75);
        assert_eq!(
            reloaded_session.viewport.canvas_background,
            CanvasBackground::Grid
        );
        assert!(!reloaded_session.settings.auto_image_scaling);
        assert_eq!(reloaded_session.settings.import_gap, 36.0);
        assert_eq!(
            reloaded_session.settings.import_target_canvas_area,
            54_321.0
        );
        assert!(!reloaded_session.settings.semantic_search_enabled);
        assert_eq!(reloaded_session.settings.canvas_image_max_side, 4096);
        assert_eq!(reloaded_session.settings.rotation_snap_degrees, 30.0);
        assert!(reloaded_session.settings.default_textbox_background);
        assert!(reloaded_session.sidebar_open);
        assert_eq!(reloaded_session.sidebar_tab, "boards");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item.id, imported.item.id);
        assert_eq!(items[0].asset.id, imported.asset.id);
        assert_eq!(items[0].item.x, 42.0);
        assert_eq!(items[0].item.y, 24.0);
        assert_eq!(items[0].item.scale, 1.5);
        assert_eq!(items[0].asset.original_path, imported.asset.original_path);
        assert!(items[0].asset.original_path.exists());
        assert_eq!(
            fs::read(&items[0].asset.original_path).unwrap(),
            stored_original_bytes
        );

        let _ = fs::remove_dir_all(app_root);
    }

    #[test]
    fn new_library_has_default_non_archived_active_board() {
        let root = temp_root("snappyref-default-board-test");
        let library = Library::initialize(root.clone()).unwrap();

        let boards = library.list_boards().unwrap();
        let session = library.load_user_session().unwrap();

        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].name, "My Board");
        assert!(!boards[0].archived);
        assert_eq!(session.active_board_id, boards[0].id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_single_canvas_data_migrates_to_my_board() {
        let root = temp_root("snappyref-board-migration-test");
        let db_path = root.join("library.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE assets (
                    id INTEGER PRIMARY KEY,
                    original_path TEXT NOT NULL UNIQUE,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    file_size INTEGER NOT NULL,
                    imported_at INTEGER NOT NULL
                );
                CREATE TABLE board_items (
                    id INTEGER PRIMARY KEY,
                    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    x REAL NOT NULL,
                    y REAL NOT NULL,
                    scale REAL NOT NULL,
                    rotation REAL NOT NULL,
                    z_index INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE canvas_annotations (
                    id INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL,
                    start_x REAL NOT NULL,
                    start_y REAL NOT NULL,
                    end_x REAL NOT NULL,
                    end_y REAL NOT NULL,
                    text TEXT NOT NULL DEFAULT '',
                    text_size TEXT NOT NULL DEFAULT 'm',
                    stroke_width TEXT NOT NULL DEFAULT 'm',
                    color TEXT NOT NULL DEFAULT 'default',
                    background_enabled INTEGER NOT NULL DEFAULT 0,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE user_session (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x REAL NOT NULL,
                    camera_pan_y REAL NOT NULL,
                    camera_zoom REAL NOT NULL,
                    auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                    import_gap REAL NOT NULL DEFAULT 24.0,
                    canvas_background TEXT NOT NULL DEFAULT 'none',
                    semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                    canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                    canvas_image_max_side INTEGER NOT NULL DEFAULT 2000,
                    rotation_snap_degrees REAL NOT NULL DEFAULT 15.0,
                    default_textbox_background INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO assets (
                    id, original_path, filename, mime_type, width, height, file_size, imported_at
                ) VALUES (11, '/tmp/legacy.png', 'legacy.png', 'image/png', 100, 80, 4, 123);
                INSERT INTO board_items (
                    id, asset_id, x, y, scale, rotation, z_index, archived
                ) VALUES (21, 11, 10.0, 20.0, 0.5, 12.0, 3, 0);
                INSERT INTO canvas_annotations (
                    id, kind, start_x, start_y, end_x, end_y, text, text_size, stroke_width, color, background_enabled, archived
                ) VALUES (31, 'textbox', 1.0, 2.0, 101.0, 52.0, 'note', 'l', 's', 'sky', 1, 0);
                INSERT INTO user_session (
                    id, camera_pan_x, camera_pan_y, camera_zoom, auto_image_scaling, import_gap,
                    canvas_background, semantic_search_enabled, canvas_image_mode,
                    canvas_image_max_side, rotation_snap_degrees, default_textbox_background
                ) VALUES (1, 44.0, 55.0, 1.25, 0, 37.0, 'grid', 0, 'original', 4096, 30.0, 1);
                ",
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let boards = library.list_boards().unwrap();
        let session = library.load_user_session().unwrap();
        let items = library.load_board_items().unwrap();
        let annotations = library.load_canvas_annotations().unwrap();

        assert_eq!(boards.len(), 1);
        assert_eq!(boards[0].name, "My Board");
        assert_eq!(session.active_board_id, boards[0].id);
        assert_eq!(session.viewport.camera_pan_x, 44.0);
        assert_eq!(session.viewport.camera_pan_y, 55.0);
        assert_eq!(session.viewport.camera_zoom, 1.25);
        assert_eq!(session.viewport.canvas_background, CanvasBackground::Grid);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item.id, 21);
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].id, 31);
        assert_eq!(annotations[0].text, "note");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_canvas_data_migrates_to_unified_stack_with_annotations_above_placements() {
        let root = temp_root("snappyref-stack-order-migration-test");
        let db_path = root.join("library.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE assets (
                    id INTEGER PRIMARY KEY,
                    original_path TEXT NOT NULL UNIQUE,
                    filename TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    file_size INTEGER NOT NULL,
                    imported_at INTEGER NOT NULL
                );
                CREATE TABLE board_items (
                    id INTEGER PRIMARY KEY,
                    asset_id INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
                    x REAL NOT NULL,
                    y REAL NOT NULL,
                    scale REAL NOT NULL,
                    rotation REAL NOT NULL,
                    z_index INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE canvas_annotations (
                    id INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL,
                    start_x REAL NOT NULL,
                    start_y REAL NOT NULL,
                    end_x REAL NOT NULL,
                    end_y REAL NOT NULL,
                    text TEXT NOT NULL DEFAULT '',
                    text_size TEXT NOT NULL DEFAULT 'm',
                    stroke_width TEXT NOT NULL DEFAULT 'm',
                    color TEXT NOT NULL DEFAULT 'default',
                    background_enabled INTEGER NOT NULL DEFAULT 0,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE user_session (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x REAL NOT NULL,
                    camera_pan_y REAL NOT NULL,
                    camera_zoom REAL NOT NULL
                );
                INSERT INTO assets (
                    id, original_path, filename, mime_type, width, height, file_size, imported_at
                ) VALUES
                    (11, '/tmp/first.png', 'first.png', 'image/png', 100, 80, 4, 123),
                    (12, '/tmp/second.png', 'second.png', 'image/png', 100, 80, 4, 124);
                INSERT INTO board_items (
                    id, asset_id, x, y, scale, rotation, z_index, archived
                ) VALUES
                    (21, 11, 10.0, 20.0, 1.0, 0.0, 7, 0),
                    (22, 12, 20.0, 20.0, 1.0, 0.0, 3, 0);
                INSERT INTO canvas_annotations (
                    id, kind, start_x, start_y, end_x, end_y, text, archived
                ) VALUES
                    (31, 'textbox', 1.0, 2.0, 101.0, 52.0, 'first note', 0),
                    (32, 'arrow', 3.0, 4.0, 103.0, 54.0, '', 0);
                INSERT INTO user_session (id, camera_pan_x, camera_pan_y, camera_zoom)
                VALUES (1, 0.0, 0.0, 1.0);
                ",
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let stack = library.load_canvas_stack_order().unwrap();

        assert_eq!(
            stack,
            vec![
                CanvasStackItem::Placement(22),
                CanvasStackItem::Placement(21),
                CanvasStackItem::Annotation(31),
                CanvasStackItem::Annotation(32),
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn created_annotations_round_trip_through_canvas_stack_order() {
        let root = temp_root("snappyref-stack-order-annotation-test");
        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .store_rgba_image("placed", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let placement = library
            .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "hello",
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();

        drop(library);
        let library = Library::initialize(root.clone()).unwrap();
        let stack = library.load_canvas_stack_order().unwrap();

        assert_eq!(
            stack,
            vec![
                CanvasStackItem::Placement(placement.item.id),
                CanvasStackItem::Annotation(annotation.id),
            ]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn saved_canvas_stack_order_round_trips_for_active_board() {
        let root = temp_root("snappyref-stack-order-save-test");
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
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(10.0, 12.0),
                egui::pos2(110.0, 72.0),
                "hello",
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();

        let saved = vec![
            CanvasStackItem::Annotation(annotation.id),
            CanvasStackItem::Placement(second.item.id),
            CanvasStackItem::Placement(first.item.id),
        ];
        library.save_canvas_stack_order(&saved).unwrap();

        drop(library);
        let library = Library::initialize(root.clone()).unwrap();
        assert_eq!(library.load_canvas_stack_order().unwrap(), saved);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn active_board_persists_and_scopes_items_annotations_and_viewport() {
        let root = temp_root("snappyref-board-scope-test");
        let library = Library::initialize(root.clone()).unwrap();
        let first_board = library.list_boards().unwrap()[0].clone();
        let first_path = library
            .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let first_item = library
            .persist_imported_image(
                &first_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        let first_annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(0.0, 0.0),
                egui::pos2(50.0, 20.0),
                "first board",
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();

        let second_board = library.create_board("Second").unwrap();
        library.set_active_board(second_board.id).unwrap();
        let second_path = library
            .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let second_item = library
            .persist_imported_image(
                &second_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(20.0, 0.0),
                1.0,
            )
            .unwrap();
        let second_annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(100.0, 0.0),
                egui::pos2(150.0, 20.0),
                "second board",
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                true,
            )
            .unwrap();
        let mut session = library.load_user_session().unwrap();
        session.viewport.camera_pan_x = 200.0;
        session.viewport.camera_pan_y = 300.0;
        session.viewport.camera_zoom = 2.0;
        session.viewport.canvas_background = CanvasBackground::Dots;
        library.save_user_session(&session).unwrap();

        let second_items = library.load_board_items().unwrap();
        let second_annotations = library.load_canvas_annotations().unwrap();
        let reopened = library.load_user_session().unwrap();
        assert_eq!(reopened.active_board_id, second_board.id);
        assert_eq!(reopened.viewport.camera_pan_x, 200.0);
        assert_eq!(reopened.viewport.canvas_background, CanvasBackground::Dots);
        assert_eq!(second_items.len(), 1);
        assert_eq!(second_items[0].item.id, second_item.item.id);
        assert_eq!(second_annotations.len(), 1);
        assert_eq!(second_annotations[0].id, second_annotation.id);

        library.set_active_board(first_board.id).unwrap();
        let first_items = library.load_board_items().unwrap();
        let first_annotations = library.load_canvas_annotations().unwrap();
        let reopened_first = library.load_user_session().unwrap();
        assert_eq!(reopened_first.active_board_id, first_board.id);
        assert_eq!(reopened_first.viewport.camera_pan_x, 0.0);
        assert_eq!(
            reopened_first.viewport.canvas_background,
            CanvasBackground::None
        );
        assert_eq!(first_items.len(), 1);
        assert_eq!(first_items[0].item.id, first_item.item.id);
        assert_eq!(first_annotations.len(), 1);
        assert_eq!(first_annotations[0].id, first_annotation.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn board_rename_allows_duplicates_and_preserves_name_for_empty_edits() {
        let root = temp_root("snappyref-board-rename-test");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();

        library.rename_board(first.id, "Second").unwrap();
        library.rename_board(second.id, "   ").unwrap();

        let boards = library.list_boards().unwrap();
        assert_eq!(boards[0].name, "Second");
        assert_eq!(boards[1].name, "Second");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delete_board_archives_board_preserves_assets_and_cleans_board_content() {
        let root = temp_root("snappyref-board-delete-test");
        let library = Library::initialize(root.clone()).unwrap();
        let first = library.list_boards().unwrap()[0].clone();
        let second = library.create_board("Second").unwrap();
        let third = library.create_board("Third").unwrap();
        library.set_active_board(second.id).unwrap();
        let path = library
            .store_rgba_image("delete-board", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let item = library
            .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(10.0, 20.0), 1.0)
            .unwrap();
        let annotation = library
            .create_canvas_annotation(
                CanvasAnnotationKind::Textbox,
                egui::pos2(0.0, 0.0),
                egui::pos2(40.0, 20.0),
                "delete me",
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();

        let remaining_active = library.delete_board(second.id).unwrap();

        assert_eq!(remaining_active, first.id);
        assert_eq!(
            library.load_user_session().unwrap().active_board_id,
            first.id
        );
        assert_eq!(
            library
                .list_boards()
                .unwrap()
                .iter()
                .map(|board| board.id)
                .collect::<Vec<_>>(),
            vec![first.id, third.id]
        );
        assert!(library.load_board_items_for_board(second.id).is_err());
        assert!(
            library
                .asset_original_paths_for_assets(&[item.asset.id])
                .unwrap()
                .contains_key(&item.asset.id)
        );
        {
            let connection = Connection::open(root.join("library.db")).unwrap();
            let annotation_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM canvas_annotations WHERE id = ?1",
                    [annotation.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(annotation_count, 0);
        }

        assert!(library.delete_board(first.id).is_ok());
        assert!(library.delete_board(third.id).is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_assets_removes_their_placements_from_all_boards_and_preserves_files() {
        let root = temp_root("snappyref-archive-assets-test");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let first_path = library
            .store_rgba_image("first", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let second_path = library
            .store_rgba_image("second", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let first_item = library
            .persist_imported_image(
                &first_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 20.0),
                1.0,
            )
            .unwrap();
        let kept_item = library
            .persist_imported_image(
                &second_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(30.0, 40.0),
                1.0,
            )
            .unwrap();
        library.set_active_board(second.id).unwrap();
        let second_placement = library
            .create_board_item_from_asset(first_item.asset.id, egui::pos2(50.0, 60.0), 1.0, 0.0, 0)
            .unwrap();
        library
            .upsert_asset_embedding_blob(first_item.asset.id, "test-model", &[1.0, 0.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(kept_item.asset.id, "test-model", &[0.0, 1.0])
            .unwrap();

        library.archive_assets(&[first_item.asset.id]).unwrap();

        assert!(first_item.asset.original_path.exists());
        assert!(second_placement.asset.original_path.exists());
        library.set_active_board(1).unwrap();
        let first_board_items = library.load_board_items().unwrap();
        assert_eq!(first_board_items.len(), 1);
        assert_eq!(first_board_items[0].asset.id, kept_item.asset.id);
        library.set_active_board(second.id).unwrap();
        assert!(library.load_board_items().unwrap().is_empty());
        let embeddings = library.load_library_embeddings("test-model").unwrap();
        assert_eq!(
            embeddings
                .iter()
                .map(|embedding| embedding.asset_id)
                .collect::<Vec<_>>(),
            vec![kept_item.asset.id]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_settings_remain_global_across_active_boards() {
        let root = temp_root("snappyref-global-settings-test");
        let library = Library::initialize(root.clone()).unwrap();
        let first_board = library.list_boards().unwrap()[0].clone();
        let second_board = library.create_board("Second").unwrap();
        library.set_active_board(second_board.id).unwrap();

        let mut session = library.load_user_session().unwrap();
        session.settings.auto_image_scaling = false;
        session.settings.import_gap = 48.0;
        session.settings.import_target_canvas_area = 45_000.0;
        session.settings.semantic_search_enabled = false;
        session.settings.semantic_search_first_launch_prompt_seen = true;
        session.settings.canvas_image_max_side = 4096;
        session.settings.rotation_snap_degrees = 45.0;
        session.settings.default_textbox_background = true;
        session.viewport.camera_pan_x = 90.0;
        session.viewport.canvas_background = CanvasBackground::Grid;
        library.save_user_session(&session).unwrap();

        library.set_active_board(first_board.id).unwrap();
        let reopened = library.load_user_session().unwrap();

        assert_eq!(reopened.active_board_id, first_board.id);
        assert!(!reopened.settings.auto_image_scaling);
        assert_eq!(reopened.settings.import_gap, 48.0);
        assert_eq!(reopened.settings.import_target_canvas_area, 45_000.0);
        assert!(!reopened.settings.semantic_search_enabled);
        assert!(reopened.settings.semantic_search_first_launch_prompt_seen);
        assert_eq!(reopened.settings.canvas_image_max_side, 4096);
        assert_eq!(reopened.settings.rotation_snap_degrees, 45.0);
        assert!(reopened.settings.default_textbox_background);
        assert_eq!(reopened.viewport.camera_pan_x, 0.0);
        assert_eq!(reopened.viewport.canvas_background, CanvasBackground::None);

        library.set_active_board(second_board.id).unwrap();
        let second_reopened = library.load_user_session().unwrap();
        assert_eq!(second_reopened.viewport.camera_pan_x, 90.0);
        assert_eq!(
            second_reopened.viewport.canvas_background,
            CanvasBackground::Grid
        );
        assert_eq!(second_reopened.settings.import_gap, 48.0);
        assert_eq!(second_reopened.settings.import_target_canvas_area, 45_000.0);

        drop(library);
        let library = Library::initialize(root.clone()).unwrap();
        library.set_active_board(first_board.id).unwrap();
        let first_after_reopen = library.load_user_session().unwrap();
        assert_eq!(first_after_reopen.viewport.camera_pan_x, 0.0);
        assert_eq!(
            first_after_reopen.viewport.canvas_background,
            CanvasBackground::None
        );
        library.set_active_board(second_board.id).unwrap();
        let second_after_reopen = library.load_user_session().unwrap();
        assert_eq!(second_after_reopen.viewport.camera_pan_x, 90.0);
        assert_eq!(
            second_after_reopen.viewport.canvas_background,
            CanvasBackground::Grid
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn user_session_payload_takes_precedence_over_legacy_preference_columns() {
        let root = temp_root("snappyref-session-payload-precedence");
        let library = Library::initialize(root.clone()).unwrap();

        let mut session = library.load_user_session().unwrap();
        session.settings.auto_image_scaling = false;
        session.settings.import_gap = 48.0;
        session.settings.import_target_canvas_area = 45_000.0;
        session.settings.semantic_search_enabled = false;
        session.settings.canvas_image_max_side = 4096;
        session.settings.rotation_snap_degrees = 45.0;
        session.settings.default_textbox_background = true;
        session.sidebar_open = true;
        session.sidebar_tab = "boards".to_owned();
        library.save_user_session(&session).unwrap();
        drop(library);

        let connection = Connection::open(root.join("library.db")).unwrap();
        connection
            .execute(
                "UPDATE user_session
                 SET auto_image_scaling = 1,
                     import_gap = 12.0,
                     import_target_canvas_area = 12.0,
                     semantic_search_enabled = 1,
                     canvas_image_max_side = 100,
                     rotation_snap_degrees = 5.0,
                     default_textbox_background = 0,
                     sidebar_open = 0,
                     sidebar_tab = 'settings'
                 WHERE id = 1",
                [],
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let reloaded = library.load_user_session().unwrap();

        assert!(!reloaded.settings.auto_image_scaling);
        assert_eq!(reloaded.settings.import_gap, 48.0);
        assert_eq!(reloaded.settings.import_target_canvas_area, 45_000.0);
        assert!(!reloaded.settings.semantic_search_enabled);
        assert_eq!(reloaded.settings.canvas_image_max_side, 4096);
        assert_eq!(reloaded.settings.rotation_snap_degrees, 45.0);
        assert!(reloaded.settings.default_textbox_background);
        assert!(reloaded.sidebar_open);
        assert_eq!(reloaded.sidebar_tab, "boards");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_legacy_user_session_columns_backfill_payload() {
        let root = temp_root("snappyref-session-payload-backfill");
        let db_path = root.join("library.db");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE user_session (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    camera_pan_x REAL NOT NULL,
                    camera_pan_y REAL NOT NULL,
                    camera_zoom REAL NOT NULL,
                    auto_image_scaling INTEGER NOT NULL DEFAULT 1,
                    import_gap REAL NOT NULL DEFAULT 24.0,
                    import_target_canvas_area REAL NOT NULL DEFAULT 90000.0,
                    canvas_background TEXT NOT NULL DEFAULT 'none',
                    semantic_search_enabled INTEGER NOT NULL DEFAULT 1,
                    canvas_image_mode TEXT NOT NULL DEFAULT 'downscale',
                    canvas_image_max_side INTEGER NOT NULL DEFAULT 2000,
                    rotation_snap_degrees REAL NOT NULL DEFAULT 15.0,
                    default_textbox_background INTEGER NOT NULL DEFAULT 0,
                    sidebar_open INTEGER NOT NULL DEFAULT 0,
                    sidebar_tab TEXT NOT NULL DEFAULT 'settings',
                    active_board_id INTEGER NOT NULL DEFAULT 1
                );

                INSERT INTO user_session (
                    id,
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    auto_image_scaling,
                    import_gap,
                    import_target_canvas_area,
                    canvas_background,
                    semantic_search_enabled,
                    canvas_image_max_side,
                    rotation_snap_degrees,
                    default_textbox_background,
                    sidebar_open,
                    sidebar_tab,
                    active_board_id
                )
                VALUES (
                    1,
                    10.0,
                    20.0,
                    1.5,
                    0,
                    48.0,
                    45000.0,
                    'grid',
                    0,
                    4096,
                    45.0,
                    1,
                    1,
                    'boards',
                    1
                );
                ",
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let session = library.load_user_session().unwrap();

        assert_eq!(session.viewport.camera_pan_x, 10.0);
        assert_eq!(session.viewport.camera_pan_y, 20.0);
        assert_eq!(session.viewport.camera_zoom, 1.5);
        assert_eq!(session.viewport.canvas_background, CanvasBackground::Grid);
        assert!(!session.settings.auto_image_scaling);
        assert_eq!(session.settings.import_gap, 48.0);
        assert_eq!(session.settings.import_target_canvas_area, 45_000.0);
        assert!(!session.settings.semantic_search_enabled);
        assert_eq!(session.settings.canvas_image_max_side, 4096);
        assert_eq!(session.settings.rotation_snap_degrees, 45.0);
        assert!(session.settings.default_textbox_background);
        assert!(session.sidebar_open);
        assert_eq!(session.sidebar_tab, "boards");
        drop(library);

        let connection = Connection::open(root.join("library.db")).unwrap();
        let payload: Option<String> = connection
            .query_row(
                "SELECT session_payload FROM user_session WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(payload.as_deref().is_some_and(|value| !value.is_empty()));
        connection
            .execute(
                "UPDATE user_session
                 SET import_gap = 12.0,
                     sidebar_open = 0,
                     sidebar_tab = 'settings'
                 WHERE id = 1",
                [],
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let reloaded = library.load_user_session().unwrap();
        assert_eq!(reloaded.settings.import_gap, 48.0);
        assert!(reloaded.sidebar_open);
        assert_eq!(reloaded.sidebar_tab, "boards");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_user_session_rows_migrate_to_default_preferences() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-storage-test-{}",
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

        assert_eq!(session.viewport.camera_pan_x, 10.0);
        assert_eq!(session.viewport.camera_pan_y, 20.0);
        assert_eq!(session.viewport.camera_zoom, 1.5);
        assert!(session.settings.auto_image_scaling);
        assert_eq!(session.settings.import_gap, crate::data::DEFAULT_IMPORT_GAP);
        assert_eq!(
            session.settings.import_target_canvas_area,
            crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA
        );
        assert_eq!(
            session.settings.canvas_image_max_side,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE
        );
        assert_eq!(session.viewport.canvas_background, CanvasBackground::None);
        assert!(session.settings.semantic_search_enabled);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canvas_image_cache_path_is_deterministic() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-thumb-path-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .store_canvas_image_cache(42, 2000, &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let path = path.unwrap();

        assert_eq!(
            path,
            root.join("thumbs")
                .join("asset-42-canvas-v3-downscale-working-2000.jpg")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clipboard_images_get_unique_destination_paths() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-clipboard-path-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let first = library
            .store_clipboard_image(&[255, 0, 0, 255], 1, 1)
            .unwrap();
        let second = library
            .store_clipboard_image(&[0, 255, 0, 255], 1, 1)
            .unwrap();

        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn store_and_remove_canvas_image_cache_round_trips_thumb_file() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-thumb-store-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let preview_path = library
            .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
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
            "snappyref-thumb-remove-variants-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let first = library
            .store_canvas_image_cache(7, 2000, &[255, 0, 0, 255], 1, 1)
            .unwrap()
            .unwrap();
        let second = library
            .store_canvas_image_cache(7, 4000, &[255, 0, 0, 255], 1, 1)
            .unwrap()
            .unwrap();

        library.remove_asset_canvas_caches(7).unwrap();

        assert!(!first.exists());
        assert!(!second.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_backfill_includes_unplaced_library_assets() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-library-embedding-test-{}",
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
        let unplaced_path = library
            .store_rgba_image("unplaced", &[0, 0, 255, 255], 1, 1)
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
        let unplaced = library
            .persist_imported_image(
                &unplaced_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(20.0, 0.0),
                1.0,
            )
            .unwrap();

        library.archive_board_items(&[unplaced.item.id]).unwrap();
        library
            .upsert_asset_embedding_blob(ready.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();

        let jobs = library
            .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
            .unwrap();

        let job_asset_ids = jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>();
        assert_eq!(job_asset_ids, vec![pending.asset.id, unplaced.asset.id]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_backfill_skips_svg_assets() {
        let root = temp_root("snappyref-svg-embedding-backfill-test");
        let library = Library::initialize(root.clone()).unwrap();
        let raster_path = library
            .store_rgba_image("raster", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let svg_path = root.join("icon.svg");
        fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>"#,
        )
        .unwrap();

        let raster = library
            .persist_imported_image(
                &raster_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let svg = library
            .persist_imported_image(
                &svg_path,
                "image/svg+xml",
                1,
                1,
                fs::metadata(&svg_path).unwrap().len(),
                egui::pos2(1.0, 0.0),
                1.0,
            )
            .unwrap();

        let active_jobs = library
            .missing_active_board_asset_jobs_for_model("mobileclip_s0")
            .unwrap();
        let live_jobs = library
            .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
            .unwrap();

        assert_eq!(
            active_jobs
                .iter()
                .map(|job| job.asset_id)
                .collect::<Vec<_>>(),
            vec![raster.asset.id]
        );
        assert_eq!(
            live_jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>(),
            vec![raster.asset.id]
        );
        assert!(!active_jobs.iter().any(|job| job.asset_id == svg.asset.id));
        assert!(!live_jobs.iter().any(|job| job.asset_id == svg.asset.id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn active_board_embedding_backfill_jobs_include_active_placements_only() {
        let root = temp_root("snappyref-active-board-embedding-backfill-test");
        let library = Library::initialize(root.clone()).unwrap();
        let second = library.create_board("Second").unwrap();
        let active_path = library
            .store_rgba_image("active", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let ready_path = library
            .store_rgba_image("ready", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let inactive_path = library
            .store_rgba_image("inactive", &[0, 0, 255, 255], 1, 1)
            .unwrap();

        let active = library
            .persist_imported_image(
                &active_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let ready = library
            .persist_imported_image(&ready_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
            .unwrap();
        library
            .upsert_asset_embedding_blob(ready.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();

        library.set_active_board(second.id).unwrap();
        let inactive = library
            .persist_imported_image(
                &inactive_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(2.0, 0.0),
                1.0,
            )
            .unwrap();
        library.set_active_board(1).unwrap();

        let jobs = library
            .missing_active_board_asset_jobs_for_model("mobileclip_s0")
            .unwrap();

        let job_asset_ids = jobs.iter().map(|job| job.asset_id).collect::<Vec<_>>();
        assert_eq!(job_asset_ids, vec![active.asset.id]);
        assert!(!job_asset_ids.contains(&ready.asset.id));
        assert!(!job_asset_ids.contains(&inactive.asset.id));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_backfill_retries_failed_assets_after_never_tried_assets() {
        let root = temp_root("snappyref-embedding-retry-order-test");
        let library = Library::initialize(root.clone()).unwrap();
        let retry_path = library
            .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let fresh_path = library
            .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let retry = library
            .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let fresh = library
            .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
            .unwrap();

        library
            .record_retryable_asset_embedding_failure(
                retry.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure",
                1_000,
            )
            .unwrap();

        let fresh_jobs = library
            .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
            .unwrap();
        assert_eq!(
            fresh_jobs
                .iter()
                .map(|job| job.asset_id)
                .collect::<Vec<_>>(),
            vec![fresh.asset.id]
        );

        let retry_jobs = library
            .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_000)
            .unwrap();
        assert_eq!(
            retry_jobs
                .iter()
                .map(|job| job.asset_id)
                .collect::<Vec<_>>(),
            vec![retry.asset.id]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_retryable_failures_get_one_immediate_and_one_delayed_retry() {
        let root = temp_root("snappyref-embedding-retry-schedule-test");
        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();

        library
            .record_retryable_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure",
                1_000,
            )
            .unwrap();
        assert_eq!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_000)
                .unwrap()
                .len(),
            1
        );

        library
            .record_retryable_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure again",
                1_010,
            )
            .unwrap();
        assert!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_309)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_310)
                .unwrap()
                .len(),
            1
        );

        library
            .record_retryable_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure exhausted",
                1_310,
            )
            .unwrap();
        assert!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 2_000)
                .unwrap()
                .is_empty()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_retry_preserves_embedding_failure_count_until_success() {
        let root = temp_root("snappyref-pending-retry-count-test");
        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();

        library
            .record_retryable_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure",
                1_000,
            )
            .unwrap();
        library
            .upsert_asset_embedding_status(
                record.asset.id,
                "mobileclip_s0",
                768,
                EmbeddingStatus::Pending,
                None,
            )
            .unwrap();
        library
            .record_retryable_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure again",
                1_010,
            )
            .unwrap();

        assert!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_309)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 1_310)
                .unwrap()
                .len(),
            1
        );

        library
            .upsert_asset_embedding_blob(record.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();
        assert!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 2_000)
                .unwrap()
                .is_empty()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_embedding_failures_are_not_automatic_retry_jobs() {
        let root = temp_root("snappyref-terminal-embedding-failure-test");
        let library = Library::initialize(root.clone()).unwrap();
        let path = library
            .store_rgba_image("terminal", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(&path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();

        library
            .record_terminal_asset_embedding_failure(
                record.asset.id,
                "mobileclip_s0",
                768,
                "failed to open image",
            )
            .unwrap();

        assert!(
            library
                .missing_live_canvas_asset_jobs_for_model("mobileclip_s0")
                .unwrap()
                .is_empty()
        );
        assert!(
            library
                .retryable_live_canvas_asset_jobs_for_model("mobileclip_s0", 10_000)
                .unwrap()
                .is_empty()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn next_embedding_backfill_job_prefers_never_tried_assets_before_retries() {
        let root = temp_root("snappyref-next-embedding-backfill-job-test");
        let library = Library::initialize(root.clone()).unwrap();
        let retry_path = library
            .store_rgba_image("retry", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let fresh_path = library
            .store_rgba_image("fresh", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let retry = library
            .persist_imported_image(&retry_path, "image/png", 1, 1, 4, egui::pos2(0.0, 0.0), 1.0)
            .unwrap();
        let fresh = library
            .persist_imported_image(&fresh_path, "image/png", 1, 1, 4, egui::pos2(1.0, 0.0), 1.0)
            .unwrap();
        library
            .record_retryable_asset_embedding_failure(
                retry.asset.id,
                "mobileclip_s0",
                768,
                "temporary failure",
                1_000,
            )
            .unwrap();

        let job = library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(job.asset_id, fresh.asset.id);

        library
            .upsert_asset_embedding_blob(fresh.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();
        let retry_job = library
            .next_embedding_backfill_job_for_model("mobileclip_s0", 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(retry_job.asset_id, retry.asset.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cropped_relink_requires_new_asset_embedding() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-crop-embedding-relink-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let library = Library::initialize(root.clone()).unwrap();
        let original_path = library
            .store_rgba_image("original", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let cropped_path = library
            .store_rgba_image("cropped", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let record = library
            .persist_imported_image(
                &original_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(record.asset.id, "mobileclip_s0", &[1.0, 0.0])
            .unwrap();

        let relinked_asset = library
            .create_asset_and_relink_board_item(
                record.item.id,
                &cropped_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
            )
            .unwrap();

        assert_eq!(
            library
                .load_asset_embedding(relinked_asset.id, "mobileclip_s0")
                .unwrap(),
            None
        );
        assert!(
            library
                .load_active_canvas_embeddings("mobileclip_s0")
                .unwrap()
                .is_empty()
        );

        library
            .upsert_asset_embedding_blob(relinked_asset.id, "mobileclip_s0", &[0.0, 1.0])
            .unwrap();
        let embeddings = library
            .load_active_canvas_embeddings("mobileclip_s0")
            .unwrap();

        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].asset_id, relinked_asset.id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_helpers_scope_to_requested_model_key() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-embedding-model-key-test-{}",
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
            "snappyref-annotation-test-{}",
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
                CanvasTextSize::default(),
                crate::data::CanvasStrokeWidth::default(),
                crate::data::CanvasAnnotationColor::default(),
                false,
            )
            .unwrap();

        let loaded = library.load_canvas_annotations().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], created);
        assert_eq!(loaded[0].text_size, CanvasTextSize::default());

        let mut updated = loaded[0].clone();
        updated.text = "updated".to_owned();
        library.update_canvas_annotation(&updated).unwrap();

        let loaded = library.load_canvas_annotations().unwrap();
        assert_eq!(loaded[0].text, "updated");

        library.archive_canvas_annotations(&[created.id]).unwrap();
        assert!(library.load_canvas_annotations().unwrap().is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn existing_canvas_annotations_rows_migrate_text_size_to_default_m() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-annotation-migration-test-{}",
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
                CREATE TABLE canvas_annotations (
                    id INTEGER PRIMARY KEY,
                    kind TEXT NOT NULL,
                    start_x REAL NOT NULL,
                    start_y REAL NOT NULL,
                    end_x REAL NOT NULL,
                    end_y REAL NOT NULL,
                    text TEXT NOT NULL DEFAULT '',
                    archived INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO canvas_annotations (
                    id, kind, start_x, start_y, end_x, end_y, text, archived
                ) VALUES (1, 'textbox', 10.0, 12.0, 110.0, 72.0, 'hello', 0);
                ",
            )
            .unwrap();
        drop(connection);

        let library = Library::initialize(root.clone()).unwrap();
        let loaded = library.load_canvas_annotations().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].text_size, CanvasTextSize::M);

        let _ = fs::remove_dir_all(root);
    }
}
