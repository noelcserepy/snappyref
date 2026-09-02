use super::assets::{insert_asset_row, load_exact_asset_for_original};
use super::boards::{active_board_id, board_is_non_archived};
use super::stack_order::append_canvas_stack_item;
use super::*;

// The SQLite table is still named `board_items` for migration compatibility.
// This adapter translates that legacy persistence vocabulary to the domain
// object exposed everywhere else: Placement.
fn load_or_insert_exact_asset(
    transaction: &Transaction<'_>,
    stored_path: &Path,
    mime_type: &str,
    width: u32,
    height: u32,
    file_size: u64,
    original_sha256: Option<&str>,
) -> Result<Asset, String> {
    if let Some(original_sha256) = original_sha256
        && let Some(asset) = load_exact_asset_for_original(transaction, file_size, original_sha256)?
    {
        return Ok(asset);
    }

    let asset = insert_asset_row(
        transaction,
        stored_path,
        mime_type,
        width,
        height,
        file_size,
        original_sha256,
    )?;
    Ok(asset)
}

fn insert_placement_row(
    transaction: &Transaction<'_>,
    placement_id: i64,
    board_id: i64,
    asset_id: i64,
    desired_world: egui::Pos2,
    display_size: egui::Vec2,
    initial_scale: f32,
) -> Result<PlacementState, String> {
    transaction
        .execute(
            "INSERT INTO board_items (
                id,
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
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                placement_id,
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
        placement_id,
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

pub(super) fn restore_board_placement(
    transaction: &Transaction<'_>,
    board_id: i64,
    placement: &Placement,
) -> Result<(), String> {
    let content_state = placement_content_state(&placement.content);
    let content_payload = serde_json::to_string(&placement.content)
        .map_err(|error| format!("failed to encode placement content: {error}"))?;
    let lifecycle = match placement.lifecycle {
        PlacementLifecycle::Provisional => "provisional",
        PlacementLifecycle::Committed => "committed",
    };
    let changed = transaction
        .execute(
            "UPDATE board_items
             SET asset_id = ?1,
                 x = ?2,
                 y = ?3,
                 width = ?4,
                 height = ?5,
                 scale = ?6,
                 rotation = ?7,
                 z_index = ?8,
                 archived = 0,
                 placement_content_state = ?9,
                 placement_lifecycle = ?10,
                 content_resolution_intent = ?11
             WHERE id = ?12
               AND board_id = ?13",
            params![
                placement.state.asset_id,
                placement.state.geometry.center.x,
                placement.state.geometry.center.y,
                placement.state.geometry.size.x,
                placement.state.geometry.size.y,
                placement.state.geometry.scale,
                placement.state.geometry.rotation,
                placement.state.geometry.z_index,
                content_state,
                lifecycle,
                content_payload,
                placement.state.id,
                board_id,
            ],
        )
        .map_err(|error| {
            format!(
                "failed to restore placement {}: {error}",
                placement.state.id
            )
        })?;
    if changed != 1 {
        return Err(format!(
            "cannot restore missing placement {}",
            placement.state.id
        ));
    }
    Ok(())
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
    pub(crate) fn prepare_loading_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        display_size: egui::Vec2,
        source: ContentResolutionSource,
        status_message: Option<String>,
    ) -> Placement {
        let state = PlacementState::with_display_size(
            self.allocate_placement_id(),
            None,
            desired_world,
            display_size,
            initial_scale,
            0.0,
            0,
            false,
        );
        Placement {
            state,
            asset: None,
            content: PlacementContent::Loading {
                source,
                status_message,
            },
            lifecycle: PlacementLifecycle::Committed,
        }
    }

    pub(crate) fn prepare_placement_from_asset(
        &self,
        asset_id: i64,
        center_world: egui::Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
    ) -> Result<Placement, String> {
        let asset = self.load_asset(asset_id)?;
        let state = PlacementState::with_display_size(
            self.allocate_placement_id(),
            Some(asset_id),
            center_world,
            egui::vec2(asset.width as f32, asset.height as f32),
            scale,
            rotation,
            z_index,
            false,
        );
        Ok(Placement::loaded(state, asset))
    }

    #[cfg_attr(not(test), allow(dead_code))]
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

    #[cfg_attr(not(test), allow(dead_code))]
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
        self.persist_imported_image_to_board_with_hash(
            board_id,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            None,
            desired_world,
            initial_scale,
        )
    }

    pub fn persist_imported_image_to_board_with_hash(
        &self,
        board_id: i64,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        original_sha256: Option<&str>,
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
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("failed to start transaction: {error}"))?;

        let asset = load_or_insert_exact_asset(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            original_sha256,
        )?;
        let item = insert_placement_row(
            &transaction,
            self.allocate_placement_id(),
            board_id,
            asset.id,
            desired_world,
            egui::vec2(asset.width as f32, asset.height as f32),
            initial_scale,
        )?;
        append_canvas_stack_item(&transaction, board_id, CanvasStackItem::Placement(item.id))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;

        Ok(Placement::loaded(item, asset))
    }

    #[cfg_attr(not(test), allow(dead_code))]
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
        self.persist_imported_image_resolving_placement_with_hash(
            board_id,
            placement_id,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            None,
            desired_world,
            initial_scale,
        )
    }

    pub fn persist_imported_image_resolving_placement_with_hash(
        &self,
        board_id: i64,
        placement_id: Option<i64>,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        original_sha256: Option<&str>,
        desired_world: egui::Pos2,
        initial_scale: f32,
    ) -> Result<Placement, String> {
        let Some(placement_id) = placement_id else {
            return self.persist_imported_image_to_board_with_hash(
                board_id,
                stored_path,
                mime_type,
                width,
                height,
                file_size,
                original_sha256,
                desired_world,
                initial_scale,
            );
        };

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| {
                format!("failed to start placement resolution transaction: {error}")
            })?;
        let asset = load_or_insert_exact_asset(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            original_sha256,
        )?;
        let content = PlacementContent::Loaded { asset_id: asset.id };
        #[cfg(test)]
        transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1, placement_content_state = 'loaded',
                     placement_lifecycle = 'committed', content_resolution_intent = ?2,
                     width = ?3, height = ?4
                 WHERE id = ?5 AND board_id = ?6 AND archived = 0",
                params![
                    asset.id,
                    encode_loaded_placement_content(asset.id)?,
                    asset.width,
                    asset.height,
                    placement_id,
                    board_id,
                ],
            )
            .map_err(|error| format!("failed to resolve placement {placement_id}: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit placement resolution: {error}"))?;

        let item = PlacementState::with_display_size(
            placement_id,
            Some(asset.id),
            desired_world,
            egui::vec2(asset.width as f32, asset.height as f32),
            initial_scale,
            0.0,
            0,
            false,
        );

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
        let Some(mut placement) = self
            .load_placements_for_board_id(&connection, board_id)?
            .into_iter()
            .find(|placement| placement.state.id == placement_id)
        else {
            return Ok(None);
        };
        let source = match &placement.content {
            PlacementContent::Loading { source, .. } => Some(source.clone()),
            PlacementContent::Failed { source, .. } => source.clone(),
            PlacementContent::Loaded { .. } => None,
        };
        placement.content = PlacementContent::Failed {
            source,
            message,
            recoverable: true,
        };
        let content_resolution_source_payload = serde_json::to_string(&placement.content)
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
        Ok(Some(placement))
    }

    pub fn load_placements(&self) -> Result<Vec<Placement>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.load_placements_for_board_id(&connection, active_board_id)
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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

    pub(super) fn load_placements_for_board_id(
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
                let content_state: String = row.get(16)?;
                let lifecycle: String = row.get(17)?;
                let content_resolution_source_payload: Option<String> = row.get(18)?;
                let asset = if let Some(asset_id) = row.get::<_, Option<i64>>(9)? {
                    Some(Asset {
                        id: asset_id,
                        original_path: PathBuf::from(row.get::<_, String>(10)?),
                        filename: row.get(11)?,
                        mime_type: row.get(12)?,
                        width: row.get::<_, u32>(13)?,
                        height: row.get::<_, u32>(14)?,
                        file_size: row.get::<_, i64>(15)? as u64,
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

    #[cfg(test)]
    pub fn update_placement_document_state(&self, item: &PlacementState) -> Result<(), String> {
        let connection = self.connection()?;
        let loaded_content = item
            .asset_id
            .map(encode_loaded_placement_content)
            .transpose()?;
        connection
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1,
                     x = ?2,
                     y = ?3,
                     width = ?4,
                     height = ?5,
                     scale = ?6,
                     rotation = ?7,
                     z_index = ?8,
                     archived = ?9,
                     placement_content_state = CASE
                         WHEN ?1 IS NOT NULL THEN 'loaded'
                         ELSE placement_content_state
                     END,
                     content_resolution_intent = CASE
                         WHEN ?1 IS NOT NULL THEN ?10
                         ELSE content_resolution_intent
                     END
                 WHERE id = ?11",
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
                    loaded_content,
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to update placement document state: {error}"))?;

        Ok(())
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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
        let item_id = self.allocate_placement_id();

        connection
            .execute(
                "INSERT INTO board_items (
                    id,
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
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    item_id,
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
