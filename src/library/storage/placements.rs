use super::assets::{insert_asset_row, load_exact_asset_for_original};
use super::boards::{active_board_id, board_is_non_archived};
use super::connection::Library;
use super::connection::write_transaction;
use super::placement_content::StoredPlacementContent;
#[cfg(test)]
use super::stack_order::append_canvas_stack_item;
use crate::data::Asset;
#[cfg(test)]
use crate::data::CanvasStackItem;
use crate::data::ContentResolutionSource;
use crate::data::Placement;
use crate::data::PlacementContent;
use crate::data::PlacementLifecycle;
use crate::data::PlacementState;
use crate::data::RectGeometry;
use crate::placement::arrangement::PlacementArrangementInput;
use eframe::egui;
use rusqlite::Connection;
use rusqlite::Transaction;
use rusqlite::params;
use std::path::Path;
use std::path::PathBuf;

use rusqlite::OptionalExtension;

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

fn placement_content_state(content: &PlacementContent) -> &'static str {
    match content {
        PlacementContent::Loaded { .. } => "loaded",
        PlacementContent::Loading { .. } => "loading",
        PlacementContent::Failed { .. } => "failed",
    }
}

#[cfg(test)]
fn append_committed_placement(
    transaction: &Transaction<'_>,
    board_id: i64,
    placement: &Placement,
) -> Result<(), String> {
    restore_board_placement(transaction, board_id, placement)?;
    append_canvas_stack_item(
        transaction,
        board_id,
        CanvasStackItem::Placement(placement.state.id),
    )
}

pub(super) fn restore_board_placement(
    transaction: &Transaction<'_>,
    board_id: i64,
    placement: &Placement,
) -> Result<(), String> {
    let content_state = placement_content_state(&placement.content);
    let content_payload = StoredPlacementContent::encode(&placement.content)?;
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
                 archived = ?9,
                 placement_content_state = ?10,
                 placement_lifecycle = ?11,
                 content_resolution_intent = ?12
             WHERE id = ?13
               AND board_id = ?14",
            params![
                placement.asset_id(),
                placement.state.geometry.center.x,
                placement.state.geometry.center.y,
                placement.state.geometry.size.x,
                placement.state.geometry.size.y,
                placement.state.geometry.scale,
                placement.state.geometry.rotation,
                placement.state.geometry.z_index,
                if placement.state.archived {
                    1_i64
                } else {
                    0_i64
                },
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
        transaction
            .execute(
                "INSERT INTO board_items (
                    id, board_id, asset_id, x, y, width, height, scale, rotation, z_index,
                    archived, placement_content_state, placement_lifecycle,
                    content_resolution_intent
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    placement.state.id,
                    board_id,
                    placement.asset_id(),
                    placement.state.geometry.center.x,
                    placement.state.geometry.center.y,
                    placement.state.geometry.size.x,
                    placement.state.geometry.size.y,
                    placement.state.geometry.scale,
                    placement.state.geometry.rotation,
                    placement.state.geometry.z_index,
                    if placement.state.archived {
                        1_i64
                    } else {
                        0_i64
                    },
                    content_state,
                    lifecycle,
                    content_payload,
                ],
            )
            .map_err(|error| {
                format!("failed to insert placement {}: {error}", placement.state.id)
            })?;
    }
    Ok(())
}

/// `asset` is the joined Asset row for the Placement's `asset_id` column.
/// A payload that claims loaded content without a live Asset degrades to a
/// recoverable failure instead of a Placement without an image.
fn decode_placement_content(
    state: &str,
    content_resolution_source_payload: Option<&str>,
    asset: Option<Asset>,
) -> PlacementContent {
    let stored = content_resolution_source_payload.and_then(StoredPlacementContent::decode);
    match stored {
        Some(StoredPlacementContent::Loading {
            source,
            status_message,
        }) => {
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
        Some(StoredPlacementContent::Failed {
            source,
            message,
            recoverable,
        }) => PlacementContent::Failed {
            source,
            message,
            recoverable,
        },
        Some(StoredPlacementContent::Loaded { .. }) | None => match (state, asset) {
            ("loading", _) => PlacementContent::Failed {
                source: None,
                message: "Placement content could not be resumed after restart".to_owned(),
                recoverable: true,
            },
            (_, Some(asset)) => PlacementContent::Loaded { asset },
            (_, None) => super::placement_content::missing_asset_content(),
        },
    }
}

fn decode_placement_lifecycle(value: &str) -> PlacementLifecycle {
    match value {
        "provisional" => PlacementLifecycle::Provisional,
        _ => PlacementLifecycle::Committed,
    }
}

impl Library {
    pub(crate) fn new_committed_loading_placement(
        &self,
        desired_world: egui::Pos2,
        initial_scale: f32,
        display_size: egui::Vec2,
        source: ContentResolutionSource,
        status_message: Option<String>,
    ) -> Placement {
        Placement {
            state: PlacementState::with_display_size(
                self.allocate_document_id(),
                desired_world,
                display_size,
                initial_scale,
                0.0,
                0,
                false,
            ),
            content: PlacementContent::Loading {
                source,
                status_message,
            },
            lifecycle: PlacementLifecycle::Committed,
        }
    }

    pub(crate) fn prepare_committed_asset_placements(
        &self,
        provisionals: &[Placement],
    ) -> Result<Vec<(i64, Placement)>, String> {
        provisionals
            .iter()
            .map(|provisional| {
                let asset_id = provisional.asset_id().ok_or_else(|| {
                    format!(
                        "provisional placement {} has no asset",
                        provisional.state.id
                    )
                })?;
                let asset = self.load_asset(asset_id)?;
                let state = PlacementState::with_display_size(
                    self.allocate_document_id(),
                    provisional.state.geometry.center,
                    provisional.state.geometry.size,
                    provisional.state.geometry.scale,
                    provisional.state.geometry.rotation,
                    provisional.state.geometry.z_index,
                    false,
                );
                Ok((provisional.state.id, Placement::loaded(state, asset)))
            })
            .collect()
    }

    pub(crate) fn prepare_committed_loading_placements(
        &self,
        provisionals: &[Placement],
    ) -> Vec<(i64, Placement)> {
        provisionals
            .iter()
            .map(|provisional| {
                let mut committed = provisional.clone();
                committed.state.id = self.allocate_document_id();
                committed.lifecycle = PlacementLifecycle::Committed;
                (provisional.state.id, committed)
            })
            .collect()
    }

    pub(crate) fn new_placement_from_asset(
        &self,
        asset_id: i64,
        center_world: egui::Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
    ) -> Result<Placement, String> {
        let asset = self.load_asset(asset_id)?;
        let state = PlacementState::with_display_size(
            self.allocate_document_id(),
            center_world,
            egui::vec2(asset.width as f32, asset.height as f32),
            scale,
            rotation,
            z_index,
            false,
        );
        Ok(Placement::loaded(state, asset))
    }

    #[cfg(test)]
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

    #[cfg(test)]
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

    #[cfg(test)]
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
        let transaction = write_transaction(&mut connection, "placement import")?;

        let asset = load_or_insert_exact_asset(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            original_sha256,
        )?;
        let placement = Placement::loaded(
            PlacementState::with_display_size(
                self.allocate_document_id(),
                desired_world,
                egui::vec2(asset.width as f32, asset.height as f32),
                initial_scale,
                0.0,
                0,
                false,
            ),
            asset,
        );
        append_committed_placement(&transaction, board_id, &placement)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit transaction: {error}"))?;
        Ok(placement)
    }

    #[cfg(test)]
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
            return Err("import persistence requires an accepted Placement".to_owned());
        };

        let mut connection = self.connection()?;
        if !board_is_non_archived(&connection, board_id)? {
            return Err(format!(
                "cannot import to missing or archived board {board_id}"
            ));
        }
        let transaction = write_transaction(&mut connection, "placement resolution")?;
        let asset = load_or_insert_exact_asset(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            original_sha256,
        )?;
        let current_state = transaction
            .query_row(
                "SELECT id, x, y, width, height, scale, rotation, z_index, archived
                 FROM board_items
                 WHERE id = ?1
                   AND board_id = ?2",
                params![placement_id, board_id],
                |row| {
                    Ok(PlacementState::with_display_size(
                        row.get(0)?,
                        egui::pos2(row.get(1)?, row.get(2)?),
                        egui::vec2(row.get(3)?, row.get(4)?),
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get::<_, i64>(8)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("failed to inspect placement {placement_id}: {error}"))?;
        let mut item = current_state.unwrap_or_else(|| {
            PlacementState::with_display_size(
                placement_id,
                desired_world,
                egui::vec2(asset.width as f32, asset.height as f32),
                initial_scale,
                0.0,
                0,
                false,
            )
        });
        if item.archived {
            return Err(format!("placement {placement_id} no longer exists"));
        }
        item.geometry.size = RectGeometry::size_preserving_area_for_aspect(
            item.geometry.size,
            asset.width,
            asset.height,
        );
        transaction
            .commit()
            .map_err(|error| format!("failed to commit imported asset: {error}"))?;
        Ok(Placement {
            state: item,
            content: PlacementContent::Loaded { asset },
            lifecycle: PlacementLifecycle::Committed,
        })
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
        let placement = Placement {
            state: PlacementState::with_display_size(
                self.allocate_document_id(),
                desired_world,
                display_size,
                initial_scale,
                0.0,
                0,
                false,
            ),
            content,
            lifecycle: PlacementLifecycle::Committed,
        };
        self.insert_committed_placement(&placement)?;
        Ok(placement)
    }

    /// Test seam: appends a Placement to the active Board through the same
    /// row writer durability uses.
    #[cfg(test)]
    fn insert_committed_placement(&self, placement: &Placement) -> Result<(), String> {
        let mut connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let transaction = write_transaction(&mut connection, "placement")?;
        append_committed_placement(&transaction, active_board_id, placement)?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit placement transaction: {error}"))
    }

    #[cfg(test)]
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
                let item = PlacementState::with_display_size(
                    row.get(0)?,
                    egui::pos2(row.get(1)?, row.get(2)?),
                    egui::vec2(row.get(3)?, row.get(4)?),
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    false,
                );
                let content_state: String = row.get(14)?;
                let lifecycle: String = row.get(15)?;
                let content_resolution_source_payload: Option<String> = row.get(16)?;
                let asset = if let Some(asset_id) = row.get::<_, Option<i64>>(8)? {
                    Some(Asset {
                        id: asset_id,
                        original_path: PathBuf::from(row.get::<_, String>(9)?),
                        filename: row.get(10)?,
                        mime_type: row.get(11)?,
                        width: row.get::<_, u32>(12)?,
                        height: row.get::<_, u32>(13)?,
                    })
                } else {
                    None
                };
                let content = decode_placement_content(
                    &content_state,
                    content_resolution_source_payload.as_deref(),
                    asset,
                );
                let lifecycle = decode_placement_lifecycle(&lifecycle);
                Ok(Placement {
                    state: item,
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
    pub fn archive_placements(&self, placement_ids: &[i64]) -> Result<(), String> {
        if placement_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "archive")?;

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
    pub fn create_placement_from_asset(
        &self,
        asset_id: i64,
        center_world: egui::Pos2,
        scale: f32,
        rotation: f32,
        z_index: i32,
    ) -> Result<Placement, String> {
        let asset = self.load_asset(asset_id)?;
        let placement = Placement::loaded(
            PlacementState::with_display_size(
                self.allocate_document_id(),
                center_world,
                egui::vec2(asset.width as f32, asset.height as f32),
                scale,
                rotation,
                z_index,
                false,
            ),
            asset,
        );
        self.insert_committed_placement(&placement)?;
        Ok(placement)
    }
}
