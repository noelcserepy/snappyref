use super::connection::Library;
use super::connection::write_transaction;
use super::files::unix_timestamp_seconds;
use super::placement_content::StoredPlacementContent;
use crate::data::Asset;
use crate::data::ContentResolutionSource;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::params;
use rusqlite::params_from_iter;
use sha2::Digest;
use sha2::Sha256;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExactDedupeReport {
    pub hashed_assets: usize,
    pub duplicate_assets_removed: usize,
    pub placements_relinked: usize,
    /// `(duplicate_id, canonical_id)` pairs; tests read these, the app only
    /// reports counts.
    pub asset_id_remaps: Vec<(i64, i64)>,
}

impl ExactDedupeReport {
    pub fn changed(&self) -> bool {
        self.duplicate_assets_removed > 0 || self.placements_relinked > 0
    }
}

const ASSET_COLUMNS: &str = "id, original_path, filename, mime_type, width, height";

fn asset_from_row(row: &rusqlite::Row<'_>) -> Result<Asset, rusqlite::Error> {
    Ok(Asset {
        id: row.get(0)?,
        original_path: PathBuf::from(row.get::<_, String>(1)?),
        filename: row.get(2)?,
        mime_type: row.get(3)?,
        width: row.get::<_, u32>(4)?,
        height: row.get::<_, u32>(5)?,
    })
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
                id,
                original_path,
                filename,
                mime_type,
                width,
                height,
                file_size,
                imported_at,
                original_sha256
             ) VALUES (
                MAX(COALESCE((SELECT MAX(id) FROM assets), 0),
                    COALESCE((SELECT MAX(old_asset_id) FROM asset_redirects), 0)) + 1,
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
    })
}

pub(super) fn load_exact_asset_for_original(
    transaction: &Transaction<'_>,
    file_size: u64,
    original_sha256: &str,
) -> Result<Option<Asset>, String> {
    transaction
        .query_row(
            &format!(
                "SELECT {ASSET_COLUMNS}
                 FROM assets
                 WHERE archived = 0
                   AND file_size = ?1
                   AND original_sha256 = ?2
                 ORDER BY id ASC
                 LIMIT 1"
            ),
            params![file_size as i64, original_sha256],
            asset_from_row,
        )
        .optional()
        .map_err(|error| format!("failed to read exact asset for {original_sha256}: {error}"))
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
    let mut content = StoredPlacementContent::decode(payload)?;
    let changed = match &mut content {
        StoredPlacementContent::Loaded { asset_id }
        | StoredPlacementContent::Loading {
            source: ContentResolutionSource::SearchAsset { asset_id },
            ..
        }
        | StoredPlacementContent::Failed {
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
    pub(crate) fn create_asset(
        &self,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
    ) -> Result<Asset, String> {
        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "asset")?;
        let asset = insert_asset_row(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            None,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit asset transaction: {error}"))?;
        Ok(asset)
    }

    pub fn dedupe_exact_assets(
        &self,
        hash_limit: usize,
        group_limit: usize,
    ) -> Result<ExactDedupeReport, String> {
        let mut report = ExactDedupeReport {
            hashed_assets: self.backfill_asset_hashes(hash_limit)?,
            ..Default::default()
        };

        let groups = self.duplicate_asset_groups(group_limit)?;
        for group in groups {
            let group_report = self.dedupe_duplicate_asset_group(group)?;
            report.duplicate_assets_removed += group_report.duplicate_assets_removed;
            report.placements_relinked += group_report.placements_relinked;
            report.asset_id_remaps.extend(group_report.asset_id_remaps);
        }
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
        let transaction = write_transaction(&mut connection, "asset dedupe")?;

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

        // Copies outside the database still refer to the old identities. Keep
        // those identities resolvable, including across subsequent merges/restarts.
        for duplicate_id in &duplicate_ids {
            transaction
                .execute(
                    "UPDATE asset_redirects SET asset_id = ?1 WHERE asset_id = ?2",
                    params![canonical_id, duplicate_id],
                )
                .map_err(|error| format!("failed to retarget merged assets: {error}"))?;
            transaction
                .execute(
                    "INSERT INTO asset_redirects (old_asset_id, asset_id) VALUES (?1, ?2)",
                    params![duplicate_id, canonical_id],
                )
                .map_err(|error| format!("failed to preserve merged asset identity: {error}"))?;
        }

        super::durability::remap_board_history_assets(&transaction, &remaps)?;
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
            asset_id_remaps: duplicate_ids
                .into_iter()
                .map(|duplicate_id| (duplicate_id, canonical_id))
                .collect(),
        })
    }

    pub fn load_asset(&self, asset_id: i64) -> Result<Asset, String> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!(
                    "SELECT {ASSET_COLUMNS}
                     FROM assets
                     WHERE id = COALESCE((SELECT asset_id FROM asset_redirects WHERE old_asset_id = ?1), ?1)
                       AND archived = 0"
                ),
                params![asset_id],
                asset_from_row,
            )
            .map_err(|error| format!("failed to load asset {asset_id}: {error}"))
    }

    /// Live Assets for `asset_ids`, keyed by id. Archived or unknown ids are
    /// simply absent so callers can degrade per Placement.
    pub(crate) fn load_assets(&self, asset_ids: &[i64]) -> Result<HashMap<i64, Asset>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT {ASSET_COLUMNS}
                 FROM assets
                 WHERE id = COALESCE((SELECT asset_id FROM asset_redirects WHERE old_asset_id = ?1), ?1)
                   AND archived = 0"
            ))
            .map_err(|error| format!("failed to prepare asset lookup: {error}"))?;
        let mut assets = HashMap::with_capacity(asset_ids.len());
        for asset_id in asset_ids {
            if let Some(asset) = statement
                .query_row(params![asset_id], asset_from_row)
                .optional()
                .map_err(|error| format!("failed to load asset {asset_id}: {error}"))?
            {
                assets.insert(*asset_id, asset);
            }
        }
        Ok(assets)
    }

    pub fn archive_assets(&self, asset_ids: &[i64]) -> Result<(), String> {
        if asset_ids.is_empty() {
            return Ok(());
        }

        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "asset archive")?;

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

        super::groups::prune_archived_group_members(&transaction)?;

        transaction
            .commit()
            .map_err(|error| format!("failed to commit asset archive transaction: {error}"))?;

        Ok(())
    }
}

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

impl Library {
    /// Registers reusable Asset content independently of Board membership and geometry.
    /// Exact reuse removes the redundant managed original.
    pub fn register_original(
        &self,
        stored_path: &Path,
        mime_type: &str,
        width: u32,
        height: u32,
        file_size: u64,
        original_sha256: Option<&str>,
    ) -> Result<Asset, String> {
        let mut connection = self.connection()?;
        let transaction = write_transaction(&mut connection, "asset registration")?;
        let asset = load_or_insert_exact_asset(
            &transaction,
            stored_path,
            mime_type,
            width,
            height,
            file_size,
            original_sha256,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("failed to commit imported asset: {error}"))?;
        if asset.original_path != stored_path {
            match std::fs::remove_file(stored_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove duplicate original: {error}")),
            }
        }
        Ok(asset)
    }
}
