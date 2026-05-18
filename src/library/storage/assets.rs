use super::*;

pub(super) fn insert_asset_row(
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

impl Library {
    pub fn create_asset_and_relink_placement(
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
            .map_err(|error| format!("failed to relink placement {item_id}: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit relink transaction: {error}"))?;

        Ok(asset)
    }

    pub fn restore_placement_and_maybe_remove_asset(
        &self,
        item: &PlacementState,
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
                    item.center.x,
                    item.center.y,
                    item.scale,
                    item.rotation,
                    item.z_index,
                    if item.archived { 1_i64 } else { 0_i64 },
                    item.id,
                ],
            )
            .map_err(|error| format!("failed to restore placement {}: {error}", item.id))?;

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
}
