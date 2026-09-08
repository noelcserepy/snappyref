#[cfg(test)]
use super::boards::active_board_id;
#[cfg(test)]
use super::boards::board_is_non_archived;
use super::connection::Library;
use super::placement_content::StoredPlacementContent;
use crate::data::Asset;
use crate::data::ContentResolutionSource;
use crate::data::Placement;
use crate::data::PlacementContent;
use crate::data::PlacementLifecycle;
use crate::data::PlacementState;
use eframe::egui;
use rusqlite::Connection;
use rusqlite::Transaction;
use rusqlite::params;
use std::path::PathBuf;

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
    pub fn load_placements(&self) -> Result<Vec<Placement>, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        self.load_placements_for_board_id(&connection, active_board_id)
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
}

#[cfg(test)]
impl crate::library::storage::Library {
    pub(crate) fn fixture_placement(
        &self,
        board_id: i64,
        placement: crate::data::Placement,
    ) -> Result<crate::data::Placement, String> {
        let mut stack = self.load_board_document(board_id)?.stack_order;
        stack.push(crate::data::CanvasStackItem::Placement(placement.id()));
        self.fixture_board_document(
            board_id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![placement.clone()],
                stack_order: Some(stack),
                ..Default::default()
            },
        )?;
        Ok(placement)
    }
}
