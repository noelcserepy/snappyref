use super::*;
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExactDedupeReport {
    pub hashed_assets: usize,
    pub duplicate_assets_removed: usize,
    pub placements_relinked: usize,
    pub canonical_asset_ids: Vec<i64>,
    pub asset_id_remaps: Vec<(i64, i64)>,
}

impl ExactDedupeReport {
    pub fn changed(&self) -> bool {
        self.duplicate_assets_removed > 0 || self.placements_relinked > 0
    }
}

pub(super) fn insert_asset_row(
    transaction: &Transaction<'_>,
    stored_path: &Path,
    mime_type: &str,
    width: u32,
    height: u32,
    file_size: u64,
    original_sha256: Option<&str>,
) -> Result<Asset, String> {
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
                imported_at,
                original_sha256
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                original_path,
                filename,
                mime_type,
                i64::from(width),
                i64::from(height),
                file_size as i64,
                imported_at,
                original_sha256,
            ],
        )
        .map_err(|error| format!("failed to insert asset: {error}"))?;

    Ok(Asset {
        id: transaction.last_insert_rowid(),
        original_path: stored_path.to_path_buf(),
        filename,
        mime_type,
        width,
        height,
        file_size,
    })
}

pub(super) fn load_exact_asset_for_original(
    transaction: &Transaction<'_>,
    file_size: u64,
    original_sha256: &str,
) -> Result<Option<Asset>, String> {
    let mut statement = transaction
        .prepare(
            "SELECT
                id,
                original_path,
                filename,
                mime_type,
                width,
                height,
                file_size
             FROM assets
             WHERE archived = 0
               AND file_size = ?1
               AND original_sha256 = ?2
             ORDER BY id ASC
             LIMIT 1",
        )
        .map_err(|error| format!("failed to prepare exact-asset lookup: {error}"))?;
    let mut rows = statement
        .query(params![file_size as i64, original_sha256])
        .map_err(|error| format!("failed to query exact-asset lookup: {error}"))?;
    let Some(row) = rows
        .next()
        .map_err(|error| format!("failed to read exact-asset lookup: {error}"))?
    else {
        return Ok(None);
    };
    Ok(Some(Asset {
        id: row.get(0).map_err(|error| {
            format!("failed to read exact-asset id for {original_sha256}: {error}")
        })?,
        original_path: PathBuf::from(row.get::<_, String>(1).map_err(|error| {
            format!("failed to read exact-asset path for {original_sha256}: {error}")
        })?),
        filename: row.get(2).map_err(|error| {
            format!("failed to read exact-asset filename for {original_sha256}: {error}")
        })?,
        mime_type: row.get(3).map_err(|error| {
            format!("failed to read exact-asset MIME type for {original_sha256}: {error}")
        })?,
        width: row.get::<_, u32>(4).map_err(|error| {
            format!("failed to read exact-asset width for {original_sha256}: {error}")
        })?,
        height: row.get::<_, u32>(5).map_err(|error| {
            format!("failed to read exact-asset height for {original_sha256}: {error}")
        })?,
        file_size: row.get::<_, i64>(6).map_err(|error| {
            format!("failed to read exact-asset file size for {original_sha256}: {error}")
        })? as u64,
    }))
}

#[derive(Clone, Debug)]
struct AssetHashJob {
    id: i64,
    original_path: PathBuf,
}

#[derive(Clone, Debug)]
struct DuplicateAsset {
    id: i64,
    original_path: PathBuf,
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let file =
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sqlite_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn remap_placement_content_asset_ids(payload: &str, remaps: &HashMap<i64, i64>) -> Option<String> {
    let mut content = serde_json::from_str::<PlacementContent>(payload).ok()?;
    let changed = match &mut content {
        PlacementContent::Loaded { asset_id }
        | PlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id },
            ..
        }
        | PlacementContent::Failed {
            source: Some(ContentResolutionSource::SearchAsset { asset_id }),
            ..
        } => {
            if let Some(canonical_id) = remaps.get(asset_id) {
                *asset_id = *canonical_id;
                true
            } else {
                false
            }
        }
        _ => false,
    };
    changed
        .then(|| serde_json::to_string(&content).ok())
        .flatten()
}

impl Library {
    pub fn dedupe_exact_assets(
        &self,
        hash_limit: usize,
        group_limit: usize,
    ) -> Result<ExactDedupeReport, String> {
        let mut report = ExactDedupeReport::default();
        report.hashed_assets = self.backfill_asset_hashes(hash_limit)?;

        let groups = self.duplicate_asset_groups(group_limit)?;
        for group in groups {
            let group_report = self.dedupe_duplicate_asset_group(group)?;
            report.duplicate_assets_removed += group_report.duplicate_assets_removed;
            report.placements_relinked += group_report.placements_relinked;
            report
                .canonical_asset_ids
                .extend(group_report.canonical_asset_ids);
            report.asset_id_remaps.extend(group_report.asset_id_remaps);
        }
        report.canonical_asset_ids.sort_unstable();
        report.canonical_asset_ids.dedup();
        report.asset_id_remaps.sort_unstable();
        report.asset_id_remaps.dedup();
        Ok(report)
    }

    fn backfill_asset_hashes(&self, limit: usize) -> Result<usize, String> {
        if limit == 0 {
            return Ok(0);
        }
        let jobs = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare(
                    "SELECT id, original_path
                     FROM assets
                     WHERE archived = 0
                       AND original_sha256 IS NULL
                     ORDER BY id ASC
                     LIMIT ?1",
                )
                .map_err(|error| format!("failed to prepare asset-hash query: {error}"))?;
            let rows = statement
                .query_map(params![sqlite_limit(limit)], |row| {
                    Ok(AssetHashJob {
                        id: row.get(0)?,
                        original_path: PathBuf::from(row.get::<_, String>(1)?),
                    })
                })
                .map_err(|error| format!("failed to read asset-hash query: {error}"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to collect asset hash jobs: {error}"))?
        };

        let mut hashed = 0_usize;
        for job in jobs {
            let hash = sha256_file(&job.original_path)?;
            let connection = self.connection()?;
            connection
                .execute(
                    "UPDATE assets
                     SET original_sha256 = ?1
                     WHERE id = ?2
                       AND original_sha256 IS NULL",
                    params![hash, job.id],
                )
                .map_err(|error| format!("failed to store asset hash {}: {error}", job.id))?;
            hashed += 1;
        }
        Ok(hashed)
    }

    fn duplicate_asset_groups(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, Vec<DuplicateAsset>)>, String> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT original_sha256, file_size
                 FROM assets
                 WHERE archived = 0
                   AND original_sha256 IS NOT NULL
                 GROUP BY original_sha256, file_size
                 HAVING COUNT(*) > 1
                 ORDER BY MIN(id) ASC
                 LIMIT ?1",
            )
            .map_err(|error| format!("failed to prepare duplicate asset group query: {error}"))?;
        let group_keys = statement
            .query_map(params![sqlite_limit(limit)], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| format!("failed to read duplicate asset group query: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect duplicate asset groups: {error}"))?;
        drop(statement);

        let mut groups = Vec::new();
        for (hash, file_size) in group_keys {
            let mut statement = connection
                .prepare(
                    "SELECT id, original_path
                     FROM assets
                     WHERE archived = 0
                       AND original_sha256 = ?1
                       AND file_size = ?2
                     ORDER BY id ASC",
                )
                .map_err(|error| format!("failed to prepare duplicate asset query: {error}"))?;
            let assets = statement
                .query_map(params![hash, file_size], |row| {
                    Ok(DuplicateAsset {
                        id: row.get(0)?,
                        original_path: PathBuf::from(row.get::<_, String>(1)?),
                    })
                })
                .map_err(|error| format!("failed to read duplicate asset query: {error}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("failed to collect duplicate assets: {error}"))?;
            if let Some(canonical) = assets.first()
                && assets.len() > 1
            {
                groups.push((canonical.id, assets[1..].to_vec()));
            }
        }
        Ok(groups)
    }

    fn dedupe_duplicate_asset_group(
        &self,
        group: (i64, Vec<DuplicateAsset>),
    ) -> Result<ExactDedupeReport, String> {
        let (canonical_id, duplicates) = group;
        if duplicates.is_empty() {
            return Ok(ExactDedupeReport::default());
        }
        let duplicate_ids = duplicates.iter().map(|asset| asset.id).collect::<Vec<_>>();
        let duplicate_paths = duplicates
            .iter()
            .map(|asset| asset.original_path.clone())
            .collect::<Vec<_>>();
        let remaps = duplicate_ids
            .iter()
            .map(|duplicate_id| (*duplicate_id, canonical_id))
            .collect::<HashMap<_, _>>();

        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("failed to start asset dedupe transaction: {error}"))?;

        let mut payload_statement = transaction
            .prepare(
                "SELECT id, content_resolution_intent
                 FROM board_items
                 WHERE content_resolution_intent IS NOT NULL",
            )
            .map_err(|error| {
                format!("failed to prepare placement-content dedupe query: {error}")
            })?;
        let payloads = payload_statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("failed to read placement-content dedupe query: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect placement-content dedupe rows: {error}"))?;
        drop(payload_statement);

        for (placement_id, payload) in payloads {
            if let Some(updated) = remap_placement_content_asset_ids(&payload, &remaps) {
                transaction
                    .execute(
                        "UPDATE board_items
                         SET content_resolution_intent = ?1
                         WHERE id = ?2",
                        params![updated, placement_id],
                    )
                    .map_err(|error| {
                        format!("failed to remap placement content {placement_id}: {error}")
                    })?;
            }
        }

        let placeholders = std::iter::repeat_n("?", duplicate_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let relink_sql = format!(
            "UPDATE board_items
             SET asset_id = ?1
             WHERE asset_id IN ({placeholders})"
        );
        let placements_relinked = transaction
            .execute(
                &relink_sql,
                params_from_iter(
                    std::iter::once(canonical_id).chain(duplicate_ids.iter().copied()),
                ),
            )
            .map_err(|error| format!("failed to relink duplicate placements: {error}"))?;

        let delete_embeddings_sql =
            format!("DELETE FROM asset_embeddings WHERE asset_id IN ({placeholders})");
        transaction
            .execute(
                &delete_embeddings_sql,
                params_from_iter(duplicate_ids.iter().copied()),
            )
            .map_err(|error| format!("failed to delete duplicate embeddings: {error}"))?;

        let delete_sql = format!("DELETE FROM assets WHERE id IN ({placeholders})");
        let duplicate_assets_removed = transaction
            .execute(&delete_sql, params_from_iter(duplicate_ids.iter().copied()))
            .map_err(|error| format!("failed to delete duplicate assets: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit asset dedupe transaction: {error}"))?;

        for path in duplicate_paths {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove duplicate original: {error}")),
            }
        }
        for duplicate_id in &duplicate_ids {
            self.remove_asset_canvas_caches(*duplicate_id)?;
        }

        Ok(ExactDedupeReport {
            hashed_assets: 0,
            duplicate_assets_removed,
            placements_relinked,
            canonical_asset_ids: vec![canonical_id],
            asset_id_remaps: duplicate_ids
                .into_iter()
                .map(|duplicate_id| (duplicate_id, canonical_id))
                .collect(),
        })
    }

    pub fn create_asset_and_relink_placement(
        &self,
        item_id: i64,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        desired_world: egui::Pos2,
    ) -> Result<Asset, String> {
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
            None,
        )?;
        let content = encode_loaded_placement_content(asset.id)?;
        transaction
            .execute(
                "UPDATE board_items
                 SET asset_id = ?1,
                     x = ?2,
                     y = ?3,
                     placement_content_state = 'loaded',
                     content_resolution_intent = ?4
                 WHERE id = ?5",
                params![asset.id, desired_world.x, desired_world.y, content, item_id],
            )
            .map_err(|error| format!("failed to relink placement {item_id}: {error}"))?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit relink transaction: {error}"))?;

        Ok(asset)
    }

    pub fn load_asset(&self, asset_id: i64) -> Result<Asset, String> {
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
                    file_size
                 FROM assets
                 WHERE id = ?1
                   AND archived = 0",
                params![asset_id],
                |row| {
                    Ok(Asset {
                        id: row.get(0)?,
                        original_path: PathBuf::from(row.get::<_, String>(1)?),
                        filename: row.get(2)?,
                        mime_type: row.get(3)?,
                        width: row.get::<_, u32>(4)?,
                        height: row.get::<_, u32>(5)?,
                        file_size: row.get::<_, i64>(6)? as u64,
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
