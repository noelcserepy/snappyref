use super::assets::insert_asset_row;
use super::boards::{active_board_id, board_is_non_archived};
use super::stack_order::append_canvas_stack_item;
use super::*;

fn insert_placement_row(
    transaction: &Transaction<'_>,
    board_id: i64,
    asset_id: i64,
    desired_world: egui::Pos2,
    display_size: egui::Vec2,
    initial_scale: f32,
) -> Result<PlacementState, String> {
    transaction
        .execute(
            "INSERT INTO board_items (
                board_id,
                asset_id,
                x,
                y,
                scale,
                width,
                height,
                rotation,
                z_index,
                archived
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                board_id,
                asset_id,
                desired_world.x,
                desired_world.y,
                initial_scale,
                display_size.x.max(1.0),
                display_size.y.max(1.0),
                0.0_f32,
                0_i64,
                0_i64
            ],
        )
        .map_err(|error| format!("failed to insert placement: {error}"))?;

    Ok(PlacementState::with_display_size(
        transaction.last_insert_rowid(),
        Some(asset_id),
        desired_world,
        display_size,
        initial_scale,
        0.0,
        0,
        false,
    ))
}

fn placement_content_state(content: &PlacementContent) -> &'static str {
    match content {
        PlacementContent::Loaded { .. } => "loaded",
        PlacementContent::Loading { .. } => "loading",
        PlacementContent::Failed { .. } => "failed",
    }
}

fn decode_placement_content(
    state: &str,
    content_resolution_source_payload: Option<&str>,
    asset_id: i64,
) -> PlacementContent {
    if let Some(content_resolution_source_payload) = content_resolution_source_payload
        && let Ok(content) =
            serde_json::from_str::<PlacementContent>(content_resolution_source_payload)
    {
        return match content {
            PlacementContent::Loading {
                source,
                status_message,
            } => {
                let resumable = matches!(
                    source,
                    ContentResolutionSource::ExternalPath { .. }
                        | ContentResolutionSource::ExternalUrl { .. }
                        | ContentResolutionSource::SearchAsset { .. }
                );
                if resumable {
                    PlacementContent::Loading {
                        source,
                        status_message,
                    }
                } else {
                    PlacementContent::Failed {
                        source: Some(source),
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
            source: None,
            message: "Placement content could not be resumed after restart".to_owned(),
            recoverable: true,
        },
        "failed" => PlacementContent::Failed {
            source: None,
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
    ) -> Result<Placement, String> {
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
    ) -> Result<Placement, String> {
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
        let item = insert_placement_row(
            &transaction,
            board_id,
            asset.id,
            desired_world,
            egui::vec2(width as f32, height as f32),
            initial_scale,
        )?;
        append_canvas_stack_item(&transaction, board_id, CanvasStackItem::Placement(item.id))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;

        Ok(Placement::loaded(item, asset))
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
    ) -> Result<Placement, String> {
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
        let content_resolution_source_payload = serde_json::to_string(&content)
            .map_err(|error| format!("failed to encode loaded placement content: {error}"))?;
        let current_size = transaction
            .query_row(
                "SELECT width, height
                 FROM board_items
                 WHERE id = ?1
                   AND board_id = ?2
                   AND archived = 0",
                params![placement_id, board_id],
                |row| Ok(egui::vec2(row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| {
                format!("failed to load placement {placement_id} size before resolution: {error}")
            })?;
        let corrected_size =
            RectGeometry::size_preserving_area_for_aspect(current_size, width, height);
        let changed = transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1,
                     placement_content_state = 'loaded',
                     placement_lifecycle = 'committed',
                     content_resolution_intent = ?2,
                     width = ?3,
                     height = ?4
                 WHERE id = ?5
                   AND board_id = ?6
                   AND archived = 0",
                params![
                    asset.id,
                    content_resolution_source_payload,
                    corrected_size.x,
                    corrected_size.y,
                    placement_id,
                    board_id
                ],
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
                "SELECT id, asset_id, x, y, width, height, scale, rotation, z_index, archived
                 FROM board_items
                 WHERE id = ?1",
                params![placement_id],
                |row| {
                    Ok(PlacementState::with_display_size(
                        row.get(0)?,
                        row.get(1)?,
                        egui::pos2(row.get(2)?, row.get(3)?),
                        egui::vec2(row.get(4)?, row.get(5)?),
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get::<_, i64>(9)? != 0,
                    ))
                },
            )
            .map_err(|error| {
                format!("failed to load resolved placement {placement_id}: {error}")
            })?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit placement resolution: {error}"))?;

        Ok(Placement {
            state: item,
            asset: Some(asset),
            content,
            lifecycle: PlacementLifecycle::Committed,
        })
    }

    pub fn mark_placement_failed(
        &self,
        board_id: i64,
        placement_id: i64,
        message: String,
    ) -> Result<Option<Placement>, String> {
        let connection = self.connection()?;
        let existing = self
            .load_placements_for_board_id(&connection, board_id)?
            .into_iter()
            .find(|record| record.state.id == placement_id);
        let Some(mut record) = existing else {
            return Ok(None);
        };
        let source = match &record.content {
            PlacementContent::Loading { source, .. } => Some(source.clone()),
            PlacementContent::Failed { source, .. } => source.clone(),
            PlacementContent::Loaded { .. } => None,
        };
        record.content = PlacementContent::Failed {
            source,
            message,
            recoverable: true,
        };
        let content_resolution_source_payload = serde_json::to_string(&record.content)
            .map_err(|error| format!("failed to encode failed placement content: {error}"))?;
        connection
            .execute(
                "UPDATE board_items
                 SET placement_content_state = 'failed',
                     content_resolution_intent = ?1
                 WHERE id = ?2
                   AND board_id = ?3
                   AND archived = 0",
                params![content_resolution_source_payload, placement_id, board_id],
            )
            .map_err(|error| format!("failed to mark placement {placement_id} failed: {error}"))?;
        Ok(Some(record))
    }

    pub fn load_placements(&self) -> Result<Vec<Placement>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.load_placements_for_board_id(&connection, active_board_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn persist_committed_loading_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        display_width: f32,
        display_height: f32,
        source: ContentResolutionSource,
        status_message: Option<String>,
    ) -> Result<Placement, String> {
        self.persist_committed_unloaded_placement(
            desired_world,
            initial_scale,
            egui::vec2(display_width, display_height),
            PlacementContent::Loading {
                source,
                status_message,
            },
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn persist_committed_failed_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        display_width: f32,
        display_height: f32,
        source: Option<ContentResolutionSource>,
        message: String,
        recoverable: bool,
    ) -> Result<Placement, String> {
        self.persist_committed_unloaded_placement(
            desired_world,
            initial_scale,
            egui::vec2(display_width, display_height),
            PlacementContent::Failed {
                source,
                message,
                recoverable,
            },
        )
    }

    fn persist_committed_unloaded_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        display_size: egui::Vec2,
        content: PlacementContent,
    ) -> Result<Placement, String> {
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start placement transaction: {error}"))?;
        let content_state = placement_content_state(&content);
        let content_resolution_source_payload = serde_json::to_string(&content)
            .map_err(|error| format!("failed to encode placement content: {error}"))?;
        transaction
            .execute(
                "INSERT INTO board_items (
                    board_id, asset_id, x, y, scale, width, height, rotation, z_index, archived,
                    placement_content_state, placement_lifecycle, content_resolution_intent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    active_board_id,
                    rusqlite::types::Null,
                    desired_world.x,
                    desired_world.y,
                    initial_scale,
                    display_size.x.max(1.0),
                    display_size.y.max(1.0),
                    0.0_f32,
                    0_i64,
                    0_i64,
                    content_state,
                    "committed",
                    content_resolution_source_payload,
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

        let item = PlacementState::with_display_size(
            item_id,
            None,
            desired_world,
            display_size,
            initial_scale,
            0.0,
            0,
            false,
        );
        Ok(Placement {
            state: item,
            asset: None,
            content,
            lifecycle: PlacementLifecycle::Committed,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_placements_for_board(&self, board_id: i64) -> Result<Vec<Placement>, String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!("cannot load missing or archived board {board_id}"));
        }
        self.load_placements_for_board_id(&connection, board_id)
    }

    fn load_placements_for_board_id(
        &self,
        connection: &Connection,
        board_id: i64,
    ) -> Result<Vec<Placement>, String> {
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    b.asset_id,
                    b.x,
                    b.y,
                    b.width,
                    b.height,
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
                let item = PlacementState::with_display_size(
                    row.get(0)?,
                    row_asset_id,
                    egui::pos2(row.get(2)?, row.get(3)?),
                    egui::vec2(row.get(4)?, row.get(5)?),
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    false,
                );
                let content_state: String = row.get(17)?;
                let lifecycle: String = row.get(18)?;
                let content_resolution_source_payload: Option<String> = row.get(19)?;
                let asset = if let Some(asset_id) = row.get::<_, Option<i64>>(9)? {
                    Some(ImageAsset {
                        id: asset_id,
                        original_path: PathBuf::from(row.get::<_, String>(10)?),
                        filename: row.get(11)?,
                        mime_type: row.get(12)?,
                        width: row.get::<_, u32>(13)?,
                        height: row.get::<_, u32>(14)?,
                        file_size: row.get::<_, i64>(15)? as u64,
                        imported_at: row.get(16)?,
                    })
                } else {
                    None
                };
                let content = decode_placement_content(
                    &content_state,
                    content_resolution_source_payload.as_deref(),
                    item.asset_id.unwrap_or(0),
                );
                let lifecycle = decode_placement_lifecycle(&lifecycle);
                Ok(Placement {
                    state: item,
                    asset,
                    content,
                    lifecycle,
                })
            })
            .map_err(|error| format!("failed to read board-items query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-items: {error}"))
    }

    pub fn load_placement_arrangement_inputs(
        &self,
    ) -> Result<Vec<PlacementArrangementInput>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.load_placement_arrangement_inputs_for_board_id(&connection, active_board_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn load_placement_arrangement_inputs_for_board(
        &self,
        board_id: i64,
    ) -> Result<Vec<PlacementArrangementInput>, String> {
        let connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!("cannot load missing or archived board {board_id}"));
        }
        self.load_placement_arrangement_inputs_for_board_id(&connection, board_id)
    }

    fn load_placement_arrangement_inputs_for_board_id(
        &self,
        connection: &Connection,
        board_id: i64,
    ) -> Result<Vec<PlacementArrangementInput>, String> {
        let mut statement = connection
            .prepare(
                "SELECT
                    b.id,
                    b.x,
                    b.y,
                    b.width * b.scale,
                    b.height * b.scale
                 FROM board_items b
                 LEFT JOIN assets a ON a.id = b.asset_id
                 WHERE b.archived = 0
                   AND (a.id IS NULL OR a.archived = 0)
                   AND b.board_id = ?1
                 ORDER BY b.z_index ASC, b.id ASC",
            )
            .map_err(|error| format!("failed to prepare board-arrangement query: {error}"))?;

        let rows = statement
            .query_map(params![board_id], |row| {
                Ok(PlacementArrangementInput::new(
                    row.get(0)?,
                    egui::pos2(row.get(1)?, row.get(2)?),
                    egui::vec2(row.get(3)?, row.get(4)?),
                ))
            })
            .map_err(|error| format!("failed to read board-arrangement query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect board-arrangement inputs: {error}"))
    }

    pub fn update_placement_state(&self, item: &PlacementState) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE board_items
                 SET x = ?1, y = ?2, width = ?3, height = ?4, scale = ?5, rotation = ?6, z_index = ?7, archived = ?8
                 WHERE id = ?9",
                params![
                    item.geometry.center.x,
                    item.geometry.center.y,
                    item.geometry.size.x,
                    item.geometry.size.y,
                    item.geometry.scale,
                    item.geometry.rotation,
                    item.geometry.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to update placement: {error}"))?;

        Ok(())
    }

    pub fn update_placement_document_state(&self, item: &PlacementState) -> Result<(), String> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1, x = ?2, y = ?3, width = ?4, height = ?5, scale = ?6, rotation = ?7, z_index = ?8, archived = ?9
                 WHERE id = ?10",
                params![
                    item.asset_id,
                    item.geometry.center.x,
                    item.geometry.center.y,
                    item.geometry.size.x,
                    item.geometry.size.y,
                    item.geometry.scale,
                    item.geometry.rotation,
                    item.geometry.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to update placement document state: {error}"))?;

        Ok(())
    }

    pub fn archive_placements(&self, placement_ids: &[i64]) -> Result<(), String> {
        if placement_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start archive transaction: {error}"))?;

        for item_id in placement_ids {
            transaction
                .execute(
                    "UPDATE board_items SET archived = 1 WHERE id = ?1",
                    params![item_id],
                )
                .map_err(|error| format!("failed to archive placement {item_id}: {error}"))?;
            transaction
                .execute(
                    "DELETE FROM canvas_stack_items
                     WHERE item_kind = 'placement'
                       AND item_id = ?1",
                    params![item_id],
                )
                .map_err(|error| {
                    format!("failed to remove placement {item_id} from canvas stack: {error}")
                })?;
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit archive transaction: {error}"))?;

        Ok(())
    }

    pub fn restore_placements(&self, placement_ids: &[i64]) -> Result<(), String> {
        if placement_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start restore transaction: {error}"))?;

        for item_id in placement_ids {
            transaction
                .execute(
                    "UPDATE board_items
                     SET archived = 0
                     WHERE id = ?1
                       AND board_id = ?2",
                    params![item_id, active_board_id],
                )
                .map_err(|error| format!("failed to restore placement {item_id}: {error}"))?;
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

    pub fn commit_provisional_placements(
        &self,
        provisionals: &[Placement],
    ) -> Result<Vec<(i64, Placement)>, String> {
        if provisionals.is_empty() {
            return Ok(Vec::new());
        }

        let assets = provisionals
            .iter()
            .map(|placement| {
                let asset_id = placement.state.asset_id.ok_or_else(|| {
                    format!("provisional placement {} has no asset", placement.state.id)
                })?;
                self.load_asset(asset_id)
                    .map(|asset| (placement.state.id, asset))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start provisional commit transaction: {error}"))?;

        let mut committed = Vec::with_capacity(provisionals.len());
        for (provisional, (_, asset)) in provisionals.iter().zip(assets) {
            transaction
                .execute(
                    "INSERT INTO board_items (
                        board_id,
                        asset_id,
                        x,
                        y,
                        scale,
                        width,
                        height,
                        rotation,
                        z_index,
                        archived
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        active_board_id,
                        asset.id,
                        provisional.state.geometry.center.x,
                        provisional.state.geometry.center.y,
                        provisional.state.geometry.scale,
                        provisional.state.geometry.size.x,
                        provisional.state.geometry.size.y,
                        provisional.state.geometry.rotation,
                        provisional.state.geometry.z_index,
                        0_i64
                    ],
                )
                .map_err(|error| {
                    format!(
                        "failed to commit provisional placement {}: {error}",
                        provisional.state.id
                    )
                })?;

            let state = PlacementState::with_display_size(
                transaction.last_insert_rowid(),
                Some(asset.id),
                provisional.state.geometry.center,
                provisional.state.geometry.size,
                provisional.state.geometry.scale,
                provisional.state.geometry.rotation,
                provisional.state.geometry.z_index,
                false,
            );
            committed.push((provisional.state.id, Placement::loaded(state, asset)));
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit provisional placements: {error}"))?;

        Ok(committed)
    }

    pub fn commit_provisional_loading_placements(
        &self,
        provisionals: &[Placement],
    ) -> Result<Vec<(i64, Placement)>, String> {
        if provisionals.is_empty() {
            return Ok(Vec::new());
        }

        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = connection.transaction().map_err(|error| {
            format!("failed to start provisional loading commit transaction: {error}")
        })?;

        let mut committed = Vec::with_capacity(provisionals.len());
        for provisional in provisionals {
            let content_state = placement_content_state(&provisional.content);
            let content_resolution_source_payload = serde_json::to_string(&provisional.content)
                .map_err(|error| format!("failed to encode placement content: {error}"))?;
            transaction
                .execute(
                    "INSERT INTO board_items (
                        board_id,
                        asset_id,
                        x,
                        y,
                        scale,
                        width,
                        height,
                        rotation,
                        z_index,
                        archived,
                        placement_content_state,
                        placement_lifecycle,
                        content_resolution_intent
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        active_board_id,
                        rusqlite::types::Null,
                        provisional.state.geometry.center.x,
                        provisional.state.geometry.center.y,
                        provisional.state.geometry.scale,
                        provisional.state.geometry.size.x,
                        provisional.state.geometry.size.y,
                        provisional.state.geometry.rotation,
                        provisional.state.geometry.z_index,
                        0_i64,
                        content_state,
                        "committed",
                        content_resolution_source_payload,
                    ],
                )
                .map_err(|error| {
                    format!(
                        "failed to commit provisional loading placement {}: {error}",
                        provisional.state.id
                    )
                })?;

            let mut placement = provisional.clone();
            let committed_id = transaction.last_insert_rowid();
            append_canvas_stack_item(
                &transaction,
                active_board_id,
                CanvasStackItem::Placement(committed_id),
            )?;
            placement.state.id = committed_id;
            placement.lifecycle = PlacementLifecycle::Committed;
            committed.push((provisional.state.id, placement));
        }

        transaction
            .commit()
            .map_err(|error| format!("failed to commit provisional loading placements: {error}"))?;

        Ok(committed)
    }

    pub fn create_placement_from_asset(
        &self,
        asset_id: i64,
        center_world: egui::Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
    ) -> Result<Placement, String> {
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
                    width,
                    height,
                    rotation,
                    z_index,
                    archived
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    active_board_id,
                    asset_id,
                    center_world.x,
                    center_world.y,
                    scale,
                    asset.width as f32,
                    asset.height as f32,
                    rotation,
                    z_index,
                    0_i64
                ],
            )
            .map_err(|error| format!("failed to create pasted placement: {error}"))?;

        let item_id = connection.last_insert_rowid();
        append_canvas_stack_item(
            &connection,
            active_board_id,
            CanvasStackItem::Placement(item_id),
        )?;

        let item = PlacementState::with_display_size(
            item_id,
            Some(asset_id),
            center_world,
            egui::vec2(asset.width as f32, asset.height as f32),
            scale,
            rotation,
            z_index,
            false,
        );
        Ok(Placement::loaded(item, asset))
    }
}
