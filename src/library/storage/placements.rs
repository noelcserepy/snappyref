use super::assets::insert_asset_row;
use super::boards::{active_board_id, board_is_non_archived};
use super::stack_order::append_canvas_stack_item;
use super::*;

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

fn placement_content_state(content: &PlacementContent) -> &'static str {
    match content {
        PlacementContent::Loaded { .. } => "loaded",
        PlacementContent::Loading { .. } => "loading",
        PlacementContent::Failed { .. } => "failed",
    }
}

fn decode_placement_content(state: &str, payload: Option<&str>, asset_id: i64) -> PlacementContent {
    if let Some(payload) = payload
        && let Ok(content) = serde_json::from_str::<PlacementContent>(payload)
    {
        return match content {
            PlacementContent::Loading {
                intent,
                status_message,
            } => {
                let resumable = matches!(
                    intent,
                    PlacementContentIntent::ExternalPath { .. }
                        | PlacementContentIntent::ExternalUrl { .. }
                        | PlacementContentIntent::SearchAsset { .. }
                );
                if resumable {
                    PlacementContent::Loading {
                        intent,
                        status_message,
                    }
                } else {
                    PlacementContent::Failed {
                        intent: Some(intent),
                        message: "Placement content could not be resumed after restart".to_owned(),
                        recoverable: true,
                    }
                }
            }
            other => other,
        };
    }

    match state {
        "loading" => PlacementContent::Failed {
            intent: None,
            message: "Placement content could not be resumed after restart".to_owned(),
            recoverable: true,
        },
        "failed" => PlacementContent::Failed {
            intent: None,
            message: "Placement content failed to resolve".to_owned(),
            recoverable: true,
        },
        _ => PlacementContent::Loaded { asset_id },
    }
}

fn decode_placement_lifecycle(value: &str) -> PlacementLifecycle {
    match value {
        "provisional" => PlacementLifecycle::Provisional,
        _ => PlacementLifecycle::Committed,
    }
}

impl Library {
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

        Ok(BoardItemWithAsset::loaded(item, asset))
    }

    pub fn persist_imported_image_resolving_placement(
        &self,
        board_id: i64,
        placement_id: Option<i64>,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        desired_world: egui::Pos2,
        initial_scale: f32,
    ) -> Result<BoardItemWithAsset, String> {
        let Some(placement_id) = placement_id else {
            return self.persist_imported_image_to_board(
                board_id,
                stored_path,
                mime_type,
                width,
                height,
                file_size,
                desired_world,
                initial_scale,
            );
        };

        let mut connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!(
                "cannot import to missing or archived board {board_id}"
            ));
        }
        let transaction = connection.transaction().map_err(|error| {
            format!("failed to start placement resolution transaction: {error}")
        })?;
        let asset = insert_asset_row(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
        )?;
        let content = PlacementContent::Loaded { asset_id: asset.id };
        let content_payload = serde_json::to_string(&content)
            .map_err(|error| format!("failed to encode loaded placement content: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1,
                     placement_content_state = 'loaded',
                     placement_lifecycle = 'committed',
                     content_resolution_intent = ?2
                 WHERE id = ?3
                   AND board_id = ?4
                   AND archived = 0",
                params![asset.id, content_payload, placement_id, board_id],
            )
            .map_err(|error| format!("failed to resolve placement {placement_id}: {error}"))?;
        if changed == 0 {
            transaction
                .execute("DELETE FROM assets WHERE id = ?1", params![asset.id])
                .map_err(|error| format!("failed to remove stale imported asset: {error}"))?;
            transaction
                .commit()
                .map_err(|error| format!("failed to commit stale placement resolution: {error}"))?;
            return Err(format!("placement {placement_id} no longer exists"));
        }
        let item = transaction
            .query_row(
                "SELECT id, asset_id, x, y, scale, rotation, z_index, archived
                 FROM board_items
                 WHERE id = ?1",
                params![placement_id],
                |row| {
                    Ok(BoardItem {
                        id: row.get(0)?,
                        asset_id: row.get(1)?,
                        x: row.get(2)?,
                        y: row.get(3)?,
                        scale: row.get(4)?,
                        rotation: row.get(5)?,
                        z_index: row.get(6)?,
                        archived: row.get::<_, i64>(7)? != 0,
                    })
                },
            )
            .map_err(|error| {
                format!("failed to load resolved placement {placement_id}: {error}")
            })?;
        let (visual_width, visual_height) = transaction
            .query_row(
                "SELECT COALESCE(visual_width, ?2), COALESCE(visual_height, ?3)
                 FROM board_items
                 WHERE id = ?1",
                params![placement_id, width as f32, height as f32],
                |row| Ok((row.get::<_, f32>(0)?, row.get::<_, f32>(1)?)),
            )
            .map_err(|error| {
                format!("failed to load resolved placement size {placement_id}: {error}")
            })?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit placement resolution: {error}"))?;

        Ok(BoardItemWithAsset {
            item,
            asset,
            content,
            lifecycle: PlacementLifecycle::Committed,
            visual_width,
            visual_height,
        })
    }

    pub fn mark_placement_failed(
        &self,
        board_id: i64,
        placement_id: i64,
        message: String,
    ) -> Result<Option<BoardItemWithAsset>, String> {
        let connection = self.connection()?;
        let existing = self
            .load_board_items_for_board_id(&connection, board_id)?
            .into_iter()
            .find(|record| record.item.id == placement_id);
        let Some(mut record) = existing else {
            return Ok(None);
        };
        let intent = match &record.content {
            PlacementContent::Loading { intent, .. } => Some(intent.clone()),
            PlacementContent::Failed { intent, .. } => intent.clone(),
            PlacementContent::Loaded { .. } => None,
        };
        record.content = PlacementContent::Failed {
            intent,
            message,
            recoverable: true,
        };
        let content_payload = serde_json::to_string(&record.content)
            .map_err(|error| format!("failed to encode failed placement content: {error}"))?;
        connection
            .execute(
                "UPDATE board_items
                 SET placement_content_state = 'failed',
                     content_resolution_intent = ?1
                 WHERE id = ?2
                   AND board_id = ?3
                   AND archived = 0",
                params![content_payload, placement_id, board_id],
            )
            .map_err(|error| format!("failed to mark placement {placement_id} failed: {error}"))?;
        Ok(Some(record))
    }

    pub fn load_board_items(&self) -> Result<Vec<BoardItemWithAsset>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.load_board_items_for_board_id(&connection, active_board_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn persist_committed_loading_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        visual_width: f32,
        visual_height: f32,
        intent: PlacementContentIntent,
        status_message: Option<String>,
    ) -> Result<BoardItemWithAsset, String> {
        self.persist_committed_unloaded_placement(
            desired_world,
            initial_scale,
            visual_width,
            visual_height,
            PlacementContent::Loading {
                intent,
                status_message,
            },
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn persist_committed_failed_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        visual_width: f32,
        visual_height: f32,
        intent: Option<PlacementContentIntent>,
        message: String,
        recoverable: bool,
    ) -> Result<BoardItemWithAsset, String> {
        self.persist_committed_unloaded_placement(
            desired_world,
            initial_scale,
            visual_width,
            visual_height,
            PlacementContent::Failed {
                intent,
                message,
                recoverable,
            },
        )
    }

    fn persist_committed_unloaded_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        visual_width: f32,
        visual_height: f32,
        content: PlacementContent,
    ) -> Result<BoardItemWithAsset, String> {
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start placement transaction: {error}"))?;
        let content_state = placement_content_state(&content);
        let content_payload = serde_json::to_string(&content)
            .map_err(|error| format!("failed to encode placement content: {error}"))?;
        transaction
            .execute(
                "INSERT INTO board_items (
                    board_id, asset_id, x, y, scale, rotation, z_index, archived,
                    placement_content_state, placement_lifecycle, visual_width,
                    visual_height, content_resolution_intent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    active_board_id,
                    rusqlite::types::Null,
                    desired_world.x,
                    desired_world.y,
                    initial_scale,
                    0.0_f32,
                    0_i64,
                    0_i64,
                    content_state,
                    "committed",
                    visual_width,
                    visual_height,
                    content_payload,
                ],
            )
            .map_err(|error| format!("failed to insert unloaded placement: {error}"))?;
        let item_id = transaction.last_insert_rowid();
        append_canvas_stack_item(
            &transaction,
            active_board_id,
            CanvasStackItem::Placement(item_id),
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit placement transaction: {error}"))?;

        let item = BoardItem {
            id: item_id,
            asset_id: 0,
            x: desired_world.x,
            y: desired_world.y,
            scale: initial_scale,
            rotation: 0.0,
            z_index: 0,
            archived: false,
        };
        let asset = BoardItemWithAsset::placeholder_asset(
            item_id,
            format!("placement-{item_id}-{content_state}"),
            visual_width.max(1.0).round() as u32,
            visual_height.max(1.0).round() as u32,
        );
        Ok(BoardItemWithAsset {
            item,
            asset,
            content,
            lifecycle: PlacementLifecycle::Committed,
            visual_width,
            visual_height,
        })
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
                    a.imported_at,
                    COALESCE(b.placement_content_state, 'loaded'),
                    COALESCE(b.placement_lifecycle, 'committed'),
                    b.visual_width,
                    b.visual_height,
                    b.content_resolution_intent
                 FROM board_items b
                 LEFT JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 0
                   AND (a.id IS NULL OR a.archived = 0)
                   AND b.board_id = ?1
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-items query: {error}"))?;

        let rows = statement
            .query_map(params![board_id], |row| {
                let row_asset_id = row.get::<_, Option<i64>>(1)?;
                let item = BoardItem {
                    id: row.get(0)?,
                    asset_id: row_asset_id.unwrap_or(0),
                    x: row.get(2)?,
                    y: row.get(3)?,
                    scale: row.get(4)?,
                    rotation: row.get(5)?,
                    z_index: row.get(6)?,
                    archived: false,
                };
                let content_state: String = row.get(15)?;
                let lifecycle: String = row.get(16)?;
                let visual_width: Option<f32> = row.get(17)?;
                let visual_height: Option<f32> = row.get(18)?;
                let content_payload: Option<String> = row.get(19)?;
                let asset = if let Some(asset_id) = row.get::<_, Option<i64>>(7)? {
                    ImageAsset {
                        id: asset_id,
                        original_path: PathBuf::from(row.get::<_, String>(8)?),
                        filename: row.get(9)?,
                        mime_type: row.get(10)?,
                        width: row.get::<_, u32>(11)?,
                        height: row.get::<_, u32>(12)?,
                        file_size: row.get::<_, i64>(13)? as u64,
                        imported_at: row.get(14)?,
                    }
                } else {
                    BoardItemWithAsset::placeholder_asset(
                        item.id,
                        format!("placement-{}-{content_state}", item.id),
                        visual_width.unwrap_or(256.0).max(1.0).round() as u32,
                        visual_height.unwrap_or(256.0).max(1.0).round() as u32,
                    )
                };
                let content = decode_placement_content(
                    &content_state,
                    content_payload.as_deref(),
                    item.asset_id,
                );
                let lifecycle = decode_placement_lifecycle(&lifecycle);
                let visual_width = visual_width.unwrap_or(asset.width as f32);
                let visual_height = visual_height.unwrap_or(asset.height as f32);
                Ok(BoardItemWithAsset {
                    item,
                    asset,
                    content,
                    lifecycle,
                    visual_width,
                    visual_height,
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
                    COALESCE(b.visual_width, a.width) * b.scale,
                    COALESCE(b.visual_height, a.height) * b.scale
                 FROM board_items b
                 LEFT JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 0
                   AND (a.id IS NULL OR a.archived = 0)
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

    pub fn update_board_item_document_state(&self, item: &BoardItem) -> Result<(), String> {
        let connection = self.connection()?;
        connection
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
            .map_err(|error| format!("failed to update board item document state: {error}"))?;

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

    pub fn restore_board_items(&self, item_ids: &[i64]) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start restore transaction: {error}"))?;

        for item_id in item_ids {
            transaction
                .execute(
                    "UPDATE board_items
                     SET archived = 0
                     WHERE id = ?1
                       AND board_id = ?2",
                    params![item_id, active_board_id],
                )
                .map_err(|error| format!("failed to restore board item {item_id}: {error}"))?;
            append_canvas_stack_item(
                &transaction,
                active_board_id,
                CanvasStackItem::Placement(*item_id),
            )?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit restore transaction: {error}"))?;

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
                    let item = BoardItem {
                        id: row.get(0)?,
                        asset_id: row.get(1)?,
                        x: row.get(2)?,
                        y: row.get(3)?,
                        scale: row.get(4)?,
                        rotation: row.get(5)?,
                        z_index: row.get(6)?,
                        archived: false,
                    };
                    let asset = ImageAsset {
                        id: row.get(7)?,
                        original_path: PathBuf::from(row.get::<_, String>(8)?),
                        filename: row.get(9)?,
                        mime_type: row.get(10)?,
                        width: row.get::<_, u32>(11)?,
                        height: row.get::<_, u32>(12)?,
                        file_size: row.get::<_, i64>(13)? as u64,
                        imported_at: row.get(14)?,
                    };
                    Ok(BoardItemWithAsset::loaded(item, asset))
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
            let item = BoardItem {
                id: item_id,
                asset_id: source.item.asset_id,
                x: source.item.x + delta.x,
                y: source.item.y + delta.y,
                scale: source.item.scale,
                rotation: source.item.rotation,
                z_index: source.item.z_index,
                archived: false,
            };
            duplicated.push(BoardItemWithAsset::loaded(item, source.asset.clone()));
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

        let item = BoardItem {
            id: item_id,
            asset_id,
            x: center_world.x,
            y: center_world.y,
            scale,
            rotation,
            z_index,
            archived: false,
        };
        Ok(BoardItemWithAsset::loaded(item, asset))
    }
}
