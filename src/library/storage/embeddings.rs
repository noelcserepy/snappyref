use super::boards::active_board_id;
use super::connection::Library;
use super::files::unix_timestamp_seconds;
use rusqlite::params;
use rusqlite::params_from_iter;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingStatus {
    Pending,
    Ready,
}

impl EmbeddingStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageEmbeddingJob {
    pub asset_id: i64,
    pub original_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LibraryAssetEmbedding {
    pub asset_id: i64,
    pub embedding_blob: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct AssetSearchHit {
    pub asset_id: i64,
    pub similarity: f32,
}

fn encode_f32_slice(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

impl Library {
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

    pub fn next_embedding_backfill_job_for_model(
        &self,
        model_key: &str,
        now: i64,
    ) -> Result<Option<ImageEmbeddingJob>, String> {
        let connection = self.connection()?;
        let board_id = active_board_id(&connection)?;
        // Fresh active-Board work, fresh Library work, then eligible retries in the same order.
        for (retry, active_board) in [(false, true), (false, false), (true, true), (true, false)] {
            let board_join = if active_board {
                "JOIN board_items b ON b.asset_id = a.id AND b.archived = 0 AND b.board_id = ?2"
            } else {
                ""
            };
            let eligibility = if retry {
                "e.status = 'failed' AND e.failure_kind = 'retryable'
                 AND e.next_retry_at IS NOT NULL AND e.next_retry_at <= ?3"
            } else {
                "e.asset_id IS NULL"
            };
            let retry_order = if retry { "e.next_retry_at ASC," } else { "" };
            let board_order = if active_board {
                "MIN(b.z_index) ASC,"
            } else {
                ""
            };
            let sql = format!(
                "SELECT a.id, a.original_path FROM assets a
                 {board_join}
                 LEFT JOIN asset_embeddings e ON e.asset_id = a.id AND e.model_key = ?1
                 WHERE a.archived = 0 AND lower(trim(a.mime_type)) <> 'image/svg+xml'
                   AND {eligibility}
                 GROUP BY a.id, a.original_path
                 ORDER BY {retry_order} {board_order} a.id ASC LIMIT 1"
            );
            let mut statement = connection
                .prepare(&sql)
                .map_err(|error| format!("failed to prepare next embedding job: {error}"))?;
            statement
                .raw_bind_parameter(1, model_key)
                .map_err(|error| format!("failed to bind embedding model: {error}"))?;
            if active_board {
                statement
                    .raw_bind_parameter(2, board_id)
                    .map_err(|error| format!("failed to bind embedding Board: {error}"))?;
            }
            if retry {
                statement
                    .raw_bind_parameter(3, now)
                    .map_err(|error| format!("failed to bind embedding retry time: {error}"))?;
            }
            let mut rows = statement.raw_query();
            if let Some(row) = rows
                .next()
                .map_err(|error| format!("failed to read next embedding job: {error}"))?
            {
                return Ok(Some(ImageEmbeddingJob {
                    asset_id: row
                        .get(0)
                        .map_err(|error| format!("invalid embedding Asset: {error}"))?,
                    original_path: PathBuf::from(
                        row.get::<_, String>(1)
                            .map_err(|error| format!("invalid embedding source: {error}"))?,
                    ),
                }));
            }
        }
        Ok(None)
    }

    pub fn load_library_embeddings(
        &self,
        model_key: &str,
    ) -> Result<Vec<LibraryAssetEmbedding>, String> {
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
                Ok(LibraryAssetEmbedding {
                    asset_id: row.get(0)?,
                    embedding_blob: row.get(1)?,
                })
            })
            .map_err(|error| format!("failed to read library-embedding query: {error}"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to collect library embeddings: {error}"))
    }

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
}
