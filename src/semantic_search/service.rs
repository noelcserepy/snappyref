use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::{AssetSearchHit, EmbeddingStatus, Library};

use super::index::{SearchScope, decode_f32_blob, search_embeddings_for_scope};
use super::model::{EmbeddingModel, ModelLoadTiming, ModelSpec, OrtDualEncoderModel};

const IMAGE_PIVOT_CANDIDATE_LIMIT: usize = 50;

pub struct SemanticSearchConfig {
    pub enabled: bool,
    pub model: ModelSpec,
    pub canvas_image_max_side: u32,
}

#[derive(Clone, Debug)]
pub struct SearchResultGroup {
    #[allow(dead_code)]
    pub asset_id: i64,
    pub asset_preview_path: Option<PathBuf>,
    #[allow(dead_code)]
    pub asset_original_path: Option<PathBuf>,
    #[allow(dead_code)]
    pub similarity: f32,
    pub item_ids: Vec<i64>,
    #[allow(dead_code)]
    pub instance_count: usize,
}

enum IndexRequest {
    Init,
    Asset {
        asset_id: i64,
        original_path: PathBuf,
    },
    BackfillOne,
}

enum SearchRequest {
    Text {
        request_id: u64,
        scope: SearchScope,
        query: String,
        limit: usize,
    },
    ImagePivot {
        request_id: u64,
        scope: SearchScope,
        source_asset_id: i64,
        text_query: Option<String>,
        limit: usize,
    },
}

pub enum IndexResult {
    Indexed {
        asset_id: i64,
    },
    Failed {
        asset_id: Option<i64>,
        error: String,
    },
    Progress {
        message: String,
    },
    ModelLoadStarted,
    ModelLoadTiming {
        timing: ModelLoadTiming,
    },
    ModelReady,
    BackfillIndexed,
    BackfillIdle,
    BackfillFailed {
        error: String,
    },
}

pub enum SearchQueryResult {
    Completed {
        request_id: u64,
        results: Vec<SearchResultGroup>,
    },
    Failed {
        request_id: u64,
        error: String,
    },
}

enum ModelState {
    Uninitialized,
    Loading,
    Ready(Arc<OrtDualEncoderModel>),
    Failed(String),
}

#[derive(Clone, Debug)]
struct DecodedAssetEmbedding {
    asset_id: i64,
    embedding: Vec<f32>,
}

#[derive(Default)]
struct SearchEmbeddingCache {
    library_embeddings: Option<Arc<Vec<DecodedAssetEmbedding>>>,
    warming: bool,
    generation: u64,
}

pub struct SemanticSearchService {
    library: Library,
    model_key: &'static str,
    model_state: Arc<Mutex<ModelState>>,
    text_embedding_cache: Arc<Mutex<HashMap<String, Vec<f32>>>>,
    search_embedding_cache: Arc<Mutex<SearchEmbeddingCache>>,
    index_tx: mpsc::Sender<IndexRequest>,
    result_rx: mpsc::Receiver<IndexResult>,
    search_tx: mpsc::Sender<SearchRequest>,
    search_result_rx: mpsc::Receiver<SearchQueryResult>,
}

impl SemanticSearchService {
    pub fn new(library: Library, config: SemanticSearchConfig) -> Result<Self, String> {
        let (index_tx, index_rx) = mpsc::channel::<IndexRequest>();
        let (result_tx, result_rx) = mpsc::channel::<IndexResult>();
        let (search_tx, search_rx) = mpsc::channel::<SearchRequest>();
        let (search_result_tx, search_result_rx) = mpsc::channel::<SearchQueryResult>();
        let model_state = Arc::new(Mutex::new(ModelState::Uninitialized));
        let text_embedding_cache = Arc::new(Mutex::new(HashMap::new()));
        let search_embedding_cache = Arc::new(Mutex::new(SearchEmbeddingCache::default()));
        let worker_library = library.clone();
        let worker_state = model_state.clone();
        let worker_search_embedding_cache = search_embedding_cache.clone();
        let model_spec = config.model.clone();
        let canvas_image_max_side = config.canvas_image_max_side;
        let model_key = config.model.model_key;

        thread::spawn(move || {
            while let Ok(first_request) = index_rx.recv() {
                let mut helper = None;
                process_index_request(
                    first_request,
                    &worker_library,
                    &worker_state,
                    &model_spec,
                    &result_tx,
                    &worker_search_embedding_cache,
                    canvas_image_max_side,
                    &mut helper,
                );
                while let Ok(request) = index_rx.try_recv() {
                    process_index_request(
                        request,
                        &worker_library,
                        &worker_state,
                        &model_spec,
                        &result_tx,
                        &worker_search_embedding_cache,
                        canvas_image_max_side,
                        &mut helper,
                    );
                }
            }
        });
        let search_library = library.clone();
        let search_state = model_state.clone();
        let search_index_tx = index_tx.clone();
        let search_text_embedding_cache = text_embedding_cache.clone();
        let worker_search_embedding_cache = search_embedding_cache.clone();
        let search_model_key = model_key;
        thread::spawn(move || {
            while let Ok(request) = search_rx.recv() {
                let (request_id, result) = match request {
                    SearchRequest::Text {
                        request_id,
                        scope,
                        query,
                        limit,
                    } => (
                        request_id,
                        search_with_limit_in_scope_for(
                            &search_library,
                            search_model_key,
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_search_embedding_cache,
                            scope,
                            &query,
                            limit,
                        ),
                    ),
                    SearchRequest::ImagePivot {
                        request_id,
                        scope,
                        source_asset_id,
                        text_query,
                        limit,
                    } => (
                        request_id,
                        search_with_image_pivot_with_limit_in_scope_for(
                            &search_library,
                            search_model_key,
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_search_embedding_cache,
                            scope,
                            source_asset_id,
                            text_query.as_deref(),
                            limit,
                        ),
                    ),
                };

                let _ = match result {
                    Ok(results) => search_result_tx.send(SearchQueryResult::Completed {
                        request_id,
                        results,
                    }),
                    Err(error) => {
                        search_result_tx.send(SearchQueryResult::Failed { request_id, error })
                    }
                };
            }
        });

        let service = Self {
            library,
            model_key,
            model_state,
            text_embedding_cache,
            search_embedding_cache,
            index_tx,
            result_rx,
            search_tx,
            search_result_rx,
        };
        if config.enabled {
            let _ = service.index_tx.send(IndexRequest::Init);
        }
        Ok(service)
    }

    pub fn enqueue_asset_index(&self, asset_id: i64, original_path: PathBuf) -> Result<(), String> {
        if self
            .library
            .load_asset_embedding(asset_id, self.model_key)?
            .is_some()
        {
            return Ok(());
        }

        self.index_tx
            .send(IndexRequest::Asset {
                asset_id,
                original_path,
            })
            .map_err(|error| format!("failed to queue asset indexing: {error}"))
    }

    pub fn enqueue_idle_backfill_step(&self) -> Result<(), String> {
        self.index_tx
            .send(IndexRequest::BackfillOne)
            .map_err(|error| format!("failed to queue embedding backfill: {error}"))
    }

    pub fn enqueue_search_with_limit_in_scope(
        &self,
        request_id: u64,
        scope: SearchScope,
        query: &str,
        limit: usize,
    ) -> Result<(), String> {
        self.search_tx
            .send(SearchRequest::Text {
                request_id,
                scope,
                query: query.to_owned(),
                limit,
            })
            .map_err(|error| format!("failed to queue semantic search: {error}"))
    }

    pub fn enqueue_search_with_image_pivot_with_limit_in_scope(
        &self,
        request_id: u64,
        scope: SearchScope,
        source_asset_id: i64,
        text_query: Option<&str>,
        limit: usize,
    ) -> Result<(), String> {
        self.search_tx
            .send(SearchRequest::ImagePivot {
                request_id,
                scope,
                source_asset_id,
                text_query: text_query.map(ToOwned::to_owned),
                limit,
            })
            .map_err(|error| format!("failed to queue semantic image search: {error}"))
    }

    #[allow(dead_code)]
    pub fn search_with_image_pivot_with_limit(
        &self,
        source_asset_id: i64,
        text_query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResultGroup>, String> {
        self.search_with_image_pivot_with_limit_in_scope(
            SearchScope::ThisBoard,
            source_asset_id,
            text_query,
            limit,
        )
    }

    pub fn search_with_image_pivot_with_limit_in_scope(
        &self,
        scope: SearchScope,
        source_asset_id: i64,
        text_query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResultGroup>, String> {
        search_with_image_pivot_with_limit_in_scope_for(
            &self.library,
            self.model_key,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.search_embedding_cache,
            scope,
            source_asset_id,
            text_query,
            limit,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn search_with_limit(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResultGroup>, String> {
        self.search_with_limit_in_scope(SearchScope::ThisBoard, query, limit)
    }

    pub fn search_with_limit_in_scope(
        &self,
        scope: SearchScope,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResultGroup>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        search_with_limit_in_scope_for(
            &self.library,
            self.model_key,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.search_embedding_cache,
            scope,
            query,
            limit,
        )
    }

    pub fn try_recv_all(&self) -> Vec<IndexResult> {
        self.result_rx.try_iter().collect()
    }

    pub fn try_recv_search_all(&self) -> Vec<SearchQueryResult> {
        self.search_result_rx.try_iter().collect()
    }

    pub fn is_ready(&self) -> bool {
        self.model_state
            .lock()
            .map(|state| matches!(&*state, ModelState::Ready(_)))
            .unwrap_or(false)
    }

    pub fn unload_model(&self) -> bool {
        let mut unloaded = false;
        if let Ok(mut state) = self.model_state.lock()
            && matches!(&*state, ModelState::Ready(_))
        {
            *state = ModelState::Uninitialized;
            unloaded = true;
        }
        if unloaded {
            if let Ok(mut cache) = self.text_embedding_cache.lock() {
                cache.clear();
            }
            release_allocator_pressure();
        }
        unloaded
    }

    pub fn search_embedding_cache_warming(&self) -> bool {
        self.search_embedding_cache
            .lock()
            .map(|cache| cache.warming)
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
fn release_allocator_pressure() {
    unsafe extern "C" {
        fn malloc_default_zone() -> *mut std::ffi::c_void;
        fn malloc_zone_pressure_relief(
            zone: *mut std::ffi::c_void,
            goal: usize,
        ) -> std::ffi::c_ulong;
    }

    unsafe {
        let zone = malloc_default_zone();
        if !zone.is_null() {
            let _ = malloc_zone_pressure_relief(zone, 0);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn release_allocator_pressure() {}

fn search_with_image_pivot_with_limit_in_scope_for(
    library: &Library,
    model_key: &'static str,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    scope: SearchScope,
    source_asset_id: i64,
    text_query: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResultGroup>, String> {
    let phase_started_at = Instant::now();
    let source_blob = library
        .load_asset_embedding(source_asset_id, model_key)?
        .ok_or_else(|| "Source image not indexed yet".to_owned())?;
    let source_embedding = normalized_embedding_blob(&source_blob)?;
    let source_ms = phase_started_at.elapsed().as_secs_f64() * 1000.0;

    let mut hits = search_assets(
        library,
        search_embedding_cache,
        scope,
        model_key,
        &source_embedding,
        IMAGE_PIVOT_CANDIDATE_LIMIT,
    )?;
    let search_ms = phase_started_at.elapsed().as_secs_f64() * 1000.0 - source_ms;
    hits.retain(|hit| hit.asset_id != source_asset_id);

    let text_query = text_query.map(str::trim).filter(|query| !query.is_empty());
    let refine_started_at = Instant::now();
    if let Some(query) = text_query {
        let query_embedding = text_embedding(query, model_state, index_tx, text_embedding_cache)?;

        let asset_ids = hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>();
        let mut candidate_embeddings: HashMap<i64, Vec<f32>> = HashMap::new();
        for asset_id in &asset_ids {
            if let Some(blob) = library.load_asset_embedding(*asset_id, model_key)? {
                candidate_embeddings.insert(*asset_id, decode_f32_blob(&blob)?);
            }
        }

        for hit in hits.iter_mut() {
            if let Some(embedding) = candidate_embeddings.get(&hit.asset_id) {
                hit.similarity = super::index::cosine_similarity(&query_embedding, embedding)?;
            }
        }
        hits.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
    }
    let refine_ms = refine_started_at.elapsed().as_secs_f64() * 1000.0;

    hits.truncate(limit);
    let group_started_at = Instant::now();
    let hit_count = hits.len();
    let results = group_hits(library, scope, hits)?;
    if semantic_search_phase_log_enabled() {
        let group_ms = group_started_at.elapsed().as_secs_f64() * 1000.0;
        let total_ms = phase_started_at.elapsed().as_secs_f64() * 1000.0;
        let result_count = results.len();
        eprintln!(
            "[search-phase] image_pivot scope={scope:?} source_asset_id={source_asset_id} source_ms={source_ms:.2} search_ms={search_ms:.2} refine_ms={refine_ms:.2} group_ms={group_ms:.2} total_ms={total_ms:.2} hits={hit_count} results={result_count}"
        );
    }
    Ok(results)
}

fn search_with_limit_in_scope_for(
    library: &Library,
    model_key: &'static str,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    scope: SearchScope,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResultGroup>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let query_embedding = text_embedding(query, model_state, index_tx, text_embedding_cache)?;
    let hits = search_assets(
        library,
        search_embedding_cache,
        scope,
        model_key,
        &query_embedding,
        limit,
    )?;
    group_hits(library, scope, hits)
}

fn process_index_request(
    request: IndexRequest,
    worker_library: &Library,
    worker_state: &Arc<Mutex<ModelState>>,
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
    worker_search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    canvas_image_max_side: u32,
    helper: &mut Option<EmbeddingHelperClient>,
) {
    match request {
        IndexRequest::Init => {
            let _ = ensure_model(worker_state, model_spec, result_tx);
        }
        IndexRequest::Asset {
            asset_id,
            original_path,
        } => {
            if let Err(error) = index_asset(
                worker_library,
                worker_state,
                model_spec,
                result_tx,
                canvas_image_max_side,
                asset_id,
                &original_path,
                helper,
            ) {
                let _ = record_embedding_failure(worker_library, asset_id, model_spec, &error);
                let _ = result_tx.send(IndexResult::Failed {
                    asset_id: Some(asset_id),
                    error,
                });
            } else {
                clear_search_embedding_cache(worker_search_embedding_cache);
                warm_search_embedding_cache(
                    worker_library.clone(),
                    model_spec.model_key,
                    worker_search_embedding_cache.clone(),
                );
                let _ = result_tx.send(IndexResult::Indexed { asset_id });
            }
        }
        IndexRequest::BackfillOne => {
            let job = match worker_library.next_embedding_backfill_job_for_model(
                model_spec.model_key,
                unix_timestamp_seconds(),
            ) {
                Ok(job) => job,
                Err(error) => {
                    let _ = result_tx.send(IndexResult::BackfillFailed { error });
                    return;
                }
            };

            let Some(job) = job else {
                let _ = result_tx.send(IndexResult::BackfillIdle);
                return;
            };

            match index_asset(
                worker_library,
                worker_state,
                model_spec,
                result_tx,
                canvas_image_max_side,
                job.asset_id,
                &job.original_path,
                helper,
            ) {
                Ok(()) => {
                    clear_search_embedding_cache(worker_search_embedding_cache);
                    warm_search_embedding_cache(
                        worker_library.clone(),
                        model_spec.model_key,
                        worker_search_embedding_cache.clone(),
                    );
                    let _ = result_tx.send(IndexResult::BackfillIndexed);
                }
                Err(error) => {
                    let _ =
                        record_embedding_failure(worker_library, job.asset_id, model_spec, &error);
                    let _ = result_tx.send(IndexResult::BackfillFailed { error });
                }
            }
        }
    }
}

fn search_assets(
    library: &Library,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    scope: SearchScope,
    model_key: &'static str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    match scope {
        SearchScope::ThisBoard => {
            search_embeddings_for_scope(library, scope, model_key, query_embedding, limit)
        }
        SearchScope::Library => search_cached_library_embeddings(
            library,
            search_embedding_cache,
            model_key,
            query_embedding,
            limit,
        ),
    }
}

fn search_cached_library_embeddings(
    library: &Library,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    model_key: &'static str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let phase_started_at = Instant::now();
    let cached_embeddings = search_embedding_cache
        .lock()
        .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?
        .library_embeddings
        .clone();
    let cache_wait_ms = phase_started_at.elapsed().as_secs_f64() * 1000.0;
    let (embeddings, loaded_cache) = if let Some(embeddings) = cached_embeddings {
        (embeddings, false)
    } else {
        let load_started_at = Instant::now();
        let decoded_embeddings = load_decoded_library_embeddings(library, model_key)?;
        let load_ms = load_started_at.elapsed().as_secs_f64() * 1000.0;
        if semantic_search_phase_log_enabled() {
            let len = decoded_embeddings.len();
            eprintln!("[search-phase] library_cache_load_ms={load_ms:.2} embeddings={len}");
        }
        let mut cache = search_embedding_cache
            .lock()
            .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?;
        cache.warming = false;
        if cache.library_embeddings.is_none() {
            cache.library_embeddings = Some(decoded_embeddings.clone());
        }
        (decoded_embeddings, true)
    };

    let score_started_at = Instant::now();
    let hits = score_decoded_embeddings(&embeddings, query_embedding, limit)?;
    if semantic_search_phase_log_enabled() {
        let score_ms = score_started_at.elapsed().as_secs_f64() * 1000.0;
        let total_ms = phase_started_at.elapsed().as_secs_f64() * 1000.0;
        let embedding_count = embeddings.len();
        let hit_count = hits.len();
        eprintln!(
            "[search-phase] library_search cache_wait_ms={cache_wait_ms:.2} loaded_cache={loaded_cache} score_ms={score_ms:.2} total_ms={total_ms:.2} embeddings={embedding_count} hits={hit_count}"
        );
    }
    Ok(hits)
}

fn load_decoded_library_embeddings(
    library: &Library,
    model_key: &'static str,
) -> Result<Arc<Vec<DecodedAssetEmbedding>>, String> {
    library
        .load_library_embeddings(model_key)?
        .into_iter()
        .map(|row| {
            Ok(DecodedAssetEmbedding {
                asset_id: row.asset_id,
                embedding: normalized_embedding_blob(&row.embedding_blob)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Arc::new)
}

fn score_decoded_embeddings(
    embeddings: &[DecodedAssetEmbedding],
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let query_embedding = normalized_embedding(query_embedding);
    let mut hits = embeddings
        .iter()
        .map(|row| {
            Ok(AssetSearchHit {
                asset_id: row.asset_id,
                similarity: dot_product(&query_embedding, &row.embedding)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    hits.sort_by(|left, right| right.similarity.total_cmp(&left.similarity));
    hits.truncate(limit);
    Ok(hits)
}

fn normalized_embedding_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
    decode_f32_blob(blob).map(|embedding| normalized_embedding(&embedding))
}

fn normalized_embedding(embedding: &[f32]) -> Vec<f32> {
    let norm = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if norm <= f32::EPSILON {
        return vec![0.0; embedding.len()];
    }
    embedding.iter().map(|value| value / norm).collect()
}

fn dot_product(left: &[f32], right: &[f32]) -> Result<f32, String> {
    if left.len() != right.len() {
        return Err("embedding dimensions do not match".to_owned());
    }
    Ok(left
        .iter()
        .zip(right.iter())
        .map(|(lhs, rhs)| lhs * rhs)
        .sum())
}

fn clear_search_embedding_cache(search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>) {
    if let Ok(mut cache) = search_embedding_cache.lock() {
        cache.library_embeddings = None;
        cache.warming = false;
        cache.generation = cache.generation.wrapping_add(1);
    }
}

fn warm_search_embedding_cache(
    library: Library,
    model_key: &'static str,
    search_embedding_cache: Arc<Mutex<SearchEmbeddingCache>>,
) {
    let generation = match search_embedding_cache.lock() {
        Ok(mut cache) => {
            if cache.library_embeddings.is_some() || cache.warming {
                return;
            }
            cache.warming = true;
            cache.generation
        }
        Err(_) => return,
    };

    thread::spawn(move || {
        let decoded_embeddings = load_decoded_library_embeddings(&library, model_key);
        let Ok(mut cache) = search_embedding_cache.lock() else {
            return;
        };
        if cache.generation != generation {
            return;
        }
        cache.warming = false;
        if let Ok(decoded_embeddings) = decoded_embeddings {
            cache.library_embeddings = Some(decoded_embeddings);
        }
    });
}

fn semantic_search_phase_log_enabled() -> bool {
    cfg!(debug_assertions) && std::env::var_os("SNAPPYREF_SEARCH_LATENCY_LOG").is_some()
}

fn group_hits(
    library: &Library,
    scope: SearchScope,
    hits: Vec<crate::storage::AssetSearchHit>,
) -> Result<Vec<SearchResultGroup>, String> {
    let asset_ids = hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>();
    let instances = library.board_item_instances_for_assets(&asset_ids)?;
    let asset_preview_paths = library.asset_search_preview_paths_for_assets(&asset_ids);
    let asset_original_paths = library.asset_original_paths_for_assets(&asset_ids)?;

    let mut instances_by_asset: HashMap<i64, Vec<i64>> = HashMap::new();
    for instance in instances {
        instances_by_asset
            .entry(instance.asset_id)
            .or_default()
            .push(instance.item_id);
    }

    Ok(hits
        .into_iter()
        .map(|hit| {
            let item_ids = instances_by_asset.remove(&hit.asset_id).unwrap_or_default();
            SearchResultGroup {
                asset_id: hit.asset_id,
                asset_preview_path: asset_preview_paths.get(&hit.asset_id).cloned(),
                asset_original_path: asset_original_paths.get(&hit.asset_id).cloned(),
                similarity: hit.similarity,
                instance_count: item_ids.len(),
                item_ids,
            }
        })
        .filter(|group| scope.includes_unplaced_assets() || !group.item_ids.is_empty())
        .collect())
}

fn text_embedding(
    query: &str,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
) -> Result<Vec<f32>, String> {
    let query = query.trim();
    if let Some(embedding) = text_embedding_cache
        .lock()
        .map_err(|_| "semantic-search text embedding cache poisoned".to_owned())?
        .get(query)
        .cloned()
    {
        return Ok(embedding);
    }

    let model = ready_model(model_state).map_err(|error| {
        if error == "Loading model…" {
            let _ = index_tx.send(IndexRequest::Init);
        }
        error
    })?;
    let embedding = model.embed_text(query)?;
    text_embedding_cache
        .lock()
        .map_err(|_| "semantic-search text embedding cache poisoned".to_owned())?
        .insert(query.to_owned(), embedding.clone());
    Ok(embedding)
}

fn ready_model(model_state: &Arc<Mutex<ModelState>>) -> Result<Arc<OrtDualEncoderModel>, String> {
    let state = model_state
        .lock()
        .map_err(|_| "semantic-search model state poisoned".to_owned())?;
    match &*state {
        ModelState::Uninitialized | ModelState::Loading => Err("Loading model…".to_owned()),
        ModelState::Ready(model) => Ok(model.clone()),
        ModelState::Failed(error) => Err(error.clone()),
    }
}

fn index_asset(
    library: &Library,
    _model_state: &Arc<Mutex<ModelState>>,
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
    canvas_image_max_side: u32,
    asset_id: i64,
    original_path: &PathBuf,
    helper: &mut Option<EmbeddingHelperClient>,
) -> Result<(), String> {
    library.upsert_asset_embedding_status(
        asset_id,
        model_spec.model_key,
        model_spec.dimension,
        EmbeddingStatus::Pending,
        None,
    )?;
    let source_path =
        embedding_source_path(library, asset_id, original_path, canvas_image_max_side);
    let helper_will_start = helper.is_none();
    if helper_will_start {
        let _ = result_tx.send(IndexResult::ModelLoadStarted);
    }
    let embedding = embed_image_in_helper(&source_path, helper)?;
    if helper_will_start {
        let _ = result_tx.send(IndexResult::ModelReady);
    }
    library.upsert_asset_embedding_blob(asset_id, model_spec.model_key, &embedding)?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct EmbedImageHelperOutput {
    embedding: Option<Vec<f32>>,
    error: Option<String>,
}

struct EmbeddingHelperClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl EmbeddingHelperClient {
    fn new() -> Result<Self, String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to find app executable: {error}"))?;
        let mut child = Command::new(executable)
            .arg("--semantic-embed-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to run embedding helper: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "embedding helper stdin unavailable".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "embedding helper stdout unavailable".to_owned())?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn embed_image(&mut self, source_path: &PathBuf) -> Result<Vec<f32>, String> {
        serde_json::to_writer(&mut self.stdin, source_path)
            .map_err(|error| format!("failed to write embedding helper input: {error}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|error| format!("failed to finish embedding helper input: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush embedding helper input: {error}"))?;

        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read embedding helper output: {error}"))?;
        if bytes == 0 {
            let status = self.child.try_wait().ok().flatten();
            return Err(match status {
                Some(status) => format!("embedding helper exited before output with {status}"),
                None => "embedding helper closed stdout before output".to_owned(),
            });
        }
        let output: EmbedImageHelperOutput = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper output: {error}"))?;
        if let Some(error) = output.error {
            return Err(error);
        }
        output
            .embedding
            .ok_or_else(|| "embedding helper output missing embedding".to_owned())
    }
}

impl Drop for EmbeddingHelperClient {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn embed_image_in_helper(
    source_path: &PathBuf,
    helper: &mut Option<EmbeddingHelperClient>,
) -> Result<Vec<f32>, String> {
    if helper.is_none() {
        *helper = Some(EmbeddingHelperClient::new()?);
    }
    helper
        .as_mut()
        .ok_or_else(|| "embedding helper unavailable".to_owned())?
        .embed_image(source_path)
}

fn record_embedding_failure(
    library: &Library,
    asset_id: i64,
    model_spec: &ModelSpec,
    error: &str,
) -> Result<(), String> {
    if is_terminal_embedding_failure(error) {
        library.record_terminal_asset_embedding_failure(
            asset_id,
            model_spec.model_key,
            model_spec.dimension,
            error,
        )
    } else {
        library.record_retryable_asset_embedding_failure(
            asset_id,
            model_spec.model_key,
            model_spec.dimension,
            error,
            unix_timestamp_seconds(),
        )
    }
}

fn is_terminal_embedding_failure(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("failed to open image")
        || error.contains("zero-sized dimensions")
        || error.contains("unsupported svg")
}

fn unix_timestamp_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn embedding_source_path(
    library: &Library,
    asset_id: i64,
    original_path: &PathBuf,
    canvas_image_max_side: u32,
) -> PathBuf {
    library
        .canvas_image_cache_path_for_lod(
            asset_id,
            canvas_image_max_side,
            crate::data::CanvasImageLod::Working,
        )
        .filter(|path| path.is_file())
        .unwrap_or_else(|| original_path.clone())
}

fn ensure_model(
    model_state: &Arc<Mutex<ModelState>>,
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
) -> Result<Arc<OrtDualEncoderModel>, String> {
    {
        let state = model_state
            .lock()
            .map_err(|_| "semantic-search model state poisoned".to_owned())?;
        match &*state {
            ModelState::Ready(model) => return Ok(model.clone()),
            ModelState::Failed(error) => return Err(error.clone()),
            ModelState::Loading | ModelState::Uninitialized => {}
        }
    }

    {
        let mut state = model_state
            .lock()
            .map_err(|_| "semantic-search model state poisoned".to_owned())?;
        match &*state {
            ModelState::Ready(model) => return Ok(model.clone()),
            ModelState::Failed(error) => return Err(error.clone()),
            ModelState::Uninitialized => *state = ModelState::Loading,
            ModelState::Loading => return Err("Loading model…".to_owned()),
        }
    }

    let _ = result_tx.send(IndexResult::ModelLoadStarted);
    match OrtDualEncoderModel::load_with_progress_timed(model_spec.clone(), |message| {
        let _ = result_tx.send(IndexResult::Progress {
            message: message.to_owned(),
        });
    }) {
        Ok((model, timing)) => {
            let _ = result_tx.send(IndexResult::ModelLoadTiming { timing });
            let model = Arc::new(model);
            let mut state = model_state
                .lock()
                .map_err(|_| "semantic-search model state poisoned".to_owned())?;
            *state = ModelState::Ready(model.clone());
            let _ = result_tx.send(IndexResult::ModelReady);
            Ok(model)
        }
        Err(error) => {
            let mut state = model_state
                .lock()
                .map_err(|_| "semantic-search model state poisoned".to_owned())?;
            *state = ModelState::Failed(error.clone());
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IndexResult, SemanticSearchConfig, SemanticSearchService, embedding_source_path,
        is_terminal_embedding_failure,
    };
    use crate::semantic_search::DEFAULT_SEMANTIC_SEARCH_MODEL;
    use crate::semantic_search::index::SearchScope;
    use crate::semantic_search::model::{ModelSpec, ResizeStrategy};
    use crate::storage::Library;
    use eframe::egui;
    use image::imageops::FilterType;
    use std::fs;
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn library_scope_image_pivot_returns_unplaced_asset_without_creating_placement() {
        let root = temp_root("snappyref-library-scope-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let unplaced_path = library
            .store_rgba_image("unplaced", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let source = library
            .persist_imported_image(
                &source_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let unplaced = library
            .persist_imported_image(
                &unplaced_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        library.archive_board_items(&[unplaced.item.id]).unwrap();
        library
            .upsert_asset_embedding_blob(source.asset.id, "test-model", &[1.0, 0.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(unplaced.asset.id, "test-model", &[0.9, 0.1])
            .unwrap();
        let unplaced_preview_path = library
            .store_canvas_image_cache(
                unplaced.asset.id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                &[0, 255, 0, 255],
                1,
                1,
            )
            .unwrap()
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: false,
                model: test_model_spec(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();

        let results = service
            .search_with_image_pivot_with_limit_in_scope(
                SearchScope::Library,
                source.asset.id,
                None,
                5,
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].asset_id, unplaced.asset.id);
        assert_eq!(
            results[0].asset_original_path.as_deref(),
            Some(unplaced.asset.original_path.as_path())
        );
        assert_eq!(
            results[0].asset_preview_path.as_deref(),
            Some(unplaced_preview_path.as_path())
        );
        assert!(results[0].item_ids.is_empty());
        assert_eq!(results[0].instance_count, 0);
        assert_eq!(library.load_board_items().unwrap().len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn this_board_scope_image_pivot_returns_active_board_placements_only() {
        let root = temp_root("snappyref-board-scope-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let second_board = library.create_board("Second").unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let active_path = library
            .store_rgba_image("active", &[0, 255, 0, 255], 1, 1)
            .unwrap();
        let inactive_path = library
            .store_rgba_image("inactive", &[0, 0, 255, 255], 1, 1)
            .unwrap();

        let source = library
            .persist_imported_image(
                &source_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let active = library
            .persist_imported_image(
                &active_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        let inactive = library
            .persist_imported_image_to_board(
                second_board.id,
                &inactive_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(20.0, 0.0),
                1.0,
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(source.asset.id, "test-model", &[1.0, 0.0])
            .unwrap();
        library
            .upsert_asset_embedding_blob(active.asset.id, "test-model", &[0.8, 0.2])
            .unwrap();
        library
            .upsert_asset_embedding_blob(inactive.asset.id, "test-model", &[0.99, 0.01])
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: false,
                model: test_model_spec(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();

        let results = service
            .search_with_image_pivot_with_limit_in_scope(
                SearchScope::ThisBoard,
                source.asset.id,
                None,
                5,
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].asset_id, active.asset.id);
        assert_eq!(results[0].item_ids, vec![active.item.id]);
        assert_eq!(results[0].instance_count, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enqueue_asset_index_skips_assets_with_ready_embedding() {
        let root = temp_root("snappyref-ready-index-skip-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("ready", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(
                &source_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(record.asset.id, "test-model", &[1.0, 0.0])
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: false,
                model: test_model_spec(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();

        service
            .enqueue_asset_index(record.asset.id, source_path)
            .unwrap();

        assert!(service.try_recv_all().is_empty());
        let embedding = library
            .load_asset_embedding(record.asset.id, "test-model")
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::semantic_search::index::decode_f32_blob(&embedding).unwrap(),
            vec![1.0, 0.0]
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn idle_backfill_without_jobs_does_not_initialize_model() {
        let root = temp_root("snappyref-empty-backfill-no-model-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: false,
                model: test_model_spec(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();

        service.enqueue_idle_backfill_step().unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut backfill_idle = false;
        while Instant::now() < deadline {
            for result in service.try_recv_all() {
                if matches!(result, IndexResult::BackfillIdle) {
                    backfill_idle = true;
                }
            }
            if backfill_idle {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(backfill_idle, "timed out waiting for empty backfill");
        assert!(!service.is_ready());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_source_prefers_working_lod_when_present() {
        let root = temp_root("snappyref-embedding-source-lod-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let original_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .persist_imported_image(
                &original_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();

        let without_lod = embedding_source_path(
            &library,
            record.asset.id,
            &record.asset.original_path,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        );
        assert_eq!(without_lod, record.asset.original_path);

        let working_lod = library
            .store_canvas_image_cache(
                record.asset.id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                &[0, 255, 0, 255],
                1,
                1,
            )
            .unwrap()
            .unwrap();

        let with_lod = embedding_source_path(
            &library,
            record.asset.id,
            &record.asset.original_path,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        );
        assert_eq!(with_lod, working_lod);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn embedding_failure_classification_keeps_corrupt_images_terminal() {
        assert!(is_terminal_embedding_failure(
            "failed to open image /tmp/bad.jpg: Format error decoding Jpeg"
        ));
        assert!(is_terminal_embedding_failure(
            "image /tmp/empty.png has zero-sized dimensions"
        ));
        assert!(!is_terminal_embedding_failure(
            "failed to run vision model: session unavailable"
        ));
        assert!(!is_terminal_embedding_failure(
            "failed to write embedding row: database is locked"
        ));
    }

    fn test_model_spec() -> ModelSpec {
        ModelSpec {
            model_key: "test-model",
            dimension: 2,
            input_size: 1,
            repo_id: "test",
            text_model_file: "text.onnx",
            vision_model_file: "vision.onnx",
            tokenizer_file: "tokenizer.json",
            token_length: 1,
            pad_id: 0,
            pad_token: "<pad>",
            text_output_name: "text",
            vision_output_name: "vision",
            normalize_mean: None,
            normalize_std: None,
            resize_filter: FilterType::Nearest,
            resize_strategy: ResizeStrategy::Exact,
        }
    }

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "{}-{}",
            prefix,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    #[ignore = "downloads the active retrieval model and a sample image for live smoke validation"]
    fn live_smoke_indexes_and_queries_single_image() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-semantic-search-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("cats.jpg");
        let curl_status = Command::new("curl")
            .args([
                "-L",
                "https://huggingface.co/datasets/Xenova/transformers.js-docs/resolve/main/cats.jpg",
                "-o",
                source_path.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(curl_status.success());

        let library = Library::initialize(root.join("library")).unwrap();
        let stored_path = library.store_original(&source_path).unwrap();
        let (width, height) = image::image_dimensions(&stored_path).unwrap();
        let file_size = fs::metadata(&stored_path).unwrap().len();
        let record = library
            .persist_imported_image(
                &stored_path,
                "image/jpeg",
                width,
                height,
                file_size,
                egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                enabled: true,
                model: DEFAULT_SEMANTIC_SEARCH_MODEL,
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();
        service
            .enqueue_asset_index(record.asset.id, stored_path.clone())
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(180);
        let mut indexed = false;
        while Instant::now() < deadline {
            for result in service.try_recv_all() {
                match result {
                    IndexResult::Indexed { .. } => indexed = true,
                    IndexResult::Failed { error, .. } => panic!("{error}"),
                    IndexResult::Progress { .. } => {}
                    IndexResult::ModelLoadStarted | IndexResult::ModelLoadTiming { .. } => {}
                    IndexResult::ModelReady => {}
                    IndexResult::BackfillIndexed
                    | IndexResult::BackfillIdle
                    | IndexResult::BackfillFailed { .. } => {}
                }
            }

            if indexed {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(indexed, "timed out waiting for semantic-search indexing");

        let results = service.search_with_limit("cat", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].asset_id, record.asset.id);

        let _ = fs::remove_dir_all(root);
    }
}
