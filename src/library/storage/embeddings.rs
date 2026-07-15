use super::boards::active_board_id;
use super::*;

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingStatus {
    Pending,
    Ready,
}

impl EmbeddingStatus {
    #[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
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
fn encode_f32_slice(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

impl Library {
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
