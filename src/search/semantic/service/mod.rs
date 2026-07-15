use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::library::storage::{AssetSearchHit, EmbeddingStatus, Library};

use super::EmbedModelLoadTimingOutput;
use super::index::decode_f32_blob;
use super::model::{EmbeddingModel, ModelLoadTiming, ModelSpec, OrtDualEncoderModel};

const IMAGE_PIVOT_CANDIDATE_LIMIT: usize = 50;

pub struct SemanticSearchConfig {
    pub enabled: bool,
    pub model: ModelSpec,
    pub canvas_image_max_side: u32,
}

#[derive(Clone, Debug)]
pub struct SearchResultAssetEntry {
    #[allow(dead_code)]
    pub asset_id: i64,
    pub asset_preview_path: Option<PathBuf>,
    pub asset_width: u32,
    pub asset_height: u32,
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
        query: String,
        limit: usize,
    },
    ImagePivot {
        request_id: u64,
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
    ImportEmbeddingStarted {
        asset_id: i64,
    },
    BackfillIndexed,
    BackfillIdle,
    BackfillFailed {
        error: String,
    },
}

pub enum SearchQueryResult {
    Completed {
        request_id: u64,
        results: Vec<SearchResultAssetEntry>,
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
            let grace = embedding_helper_grace();
            run_index_worker_loop(&index_rx, grace, |request, helper| {
                process_index_request(
                    request,
                    &worker_library,
                    &worker_state,
                    &model_spec,
                    &result_tx,
                    &worker_search_embedding_cache,
                    canvas_image_max_side,
                    helper,
                );
            });
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
                        query,
                        limit,
                    } => (
                        request_id,
                        search_with_limit_for(
                            &search_library,
                            search_model_key,
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_search_embedding_cache,
                            &query,
                            limit,
                        ),
                    ),
                    SearchRequest::ImagePivot {
                        request_id,
                        source_asset_id,
                        text_query,
                        limit,
                    } => (
                        request_id,
                        search_with_image_pivot_with_limit_for(
                            &search_library,
                            search_model_key,
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_search_embedding_cache,
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

    pub fn enqueue_search_with_limit(
        &self,
        request_id: u64,
        query: &str,
        limit: usize,
    ) -> Result<(), String> {
        self.search_tx
            .send(SearchRequest::Text {
                request_id,
                query: query.to_owned(),
                limit,
            })
            .map_err(|error| format!("failed to queue semantic search: {error}"))
    }

    pub fn enqueue_search_with_image_pivot_with_limit(
        &self,
        request_id: u64,
        source_asset_id: i64,
        text_query: Option<&str>,
        limit: usize,
    ) -> Result<(), String> {
        self.search_tx
            .send(SearchRequest::ImagePivot {
                request_id,
                source_asset_id,
                text_query: text_query.map(ToOwned::to_owned),
                limit,
            })
            .map_err(|error| format!("failed to queue semantic image search: {error}"))
    }

    #[cfg(test)]
    pub fn search_with_image_pivot_with_limit(
        &self,
        source_asset_id: i64,
        text_query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResultAssetEntry>, String> {
        search_with_image_pivot_with_limit_for(
            &self.library,
            self.model_key,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.search_embedding_cache,
            source_asset_id,
            text_query,
            limit,
        )
    }

    #[cfg(test)]
    pub fn search_with_limit(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResultAssetEntry>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        search_with_limit_for(
            &self.library,
            self.model_key,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.search_embedding_cache,
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

    pub fn invalidate_search_embedding_cache(&self) {
        clear_search_embedding_cache(&self.search_embedding_cache);
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

fn search_with_image_pivot_with_limit_for(
    library: &Library,
    model_key: &'static str,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    source_asset_id: i64,
    text_query: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResultAssetEntry>, String> {
    let source_blob = library
        .load_asset_embedding(source_asset_id, model_key)?
        .ok_or_else(|| "Source image not indexed yet".to_owned())?;
    let source_embedding = normalized_embedding_blob(&source_blob)?;

    let mut hits = search_assets(
        library,
        search_embedding_cache,
        model_key,
        &source_embedding,
        IMAGE_PIVOT_CANDIDATE_LIMIT,
    )?;
    hits.retain(|hit| hit.asset_id != source_asset_id);

    let text_query = text_query.map(str::trim).filter(|query| !query.is_empty());
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

    hits.truncate(limit);
    let results = group_hits(library, hits)?;
    Ok(results)
}

fn search_with_limit_for(
    library: &Library,
    model_key: &'static str,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResultAssetEntry>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let query_embedding = text_embedding(query, model_state, index_tx, text_embedding_cache)?;
    let hits = search_assets(
        library,
        search_embedding_cache,
        model_key,
        &query_embedding,
        limit,
    )?;
    group_hits(library, hits)
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
                model_spec,
                result_tx,
                canvas_image_max_side,
                asset_id,
                &original_path,
                helper,
                true,
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
                model_spec,
                result_tx,
                canvas_image_max_side,
                job.asset_id,
                &job.original_path,
                helper,
                false,
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
    model_key: &'static str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    search_cached_library_embeddings(
        library,
        search_embedding_cache,
        model_key,
        query_embedding,
        limit,
    )
}

fn search_cached_library_embeddings(
    library: &Library,
    search_embedding_cache: &Arc<Mutex<SearchEmbeddingCache>>,
    model_key: &'static str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<AssetSearchHit>, String> {
    let cached_embeddings = search_embedding_cache
        .lock()
        .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?
        .library_embeddings
        .clone();
    let embeddings = if let Some(embeddings) = cached_embeddings {
        embeddings
    } else {
        let decoded_embeddings = load_decoded_library_embeddings(library, model_key)?;
        let mut cache = search_embedding_cache
            .lock()
            .map_err(|_| "semantic-search embedding cache poisoned".to_owned())?;
        cache.warming = false;
        if cache.library_embeddings.is_none() {
            cache.library_embeddings = Some(decoded_embeddings.clone());
        }
        decoded_embeddings
    };

    let hits = score_decoded_embeddings(&embeddings, query_embedding, limit)?;
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

fn group_hits(
    library: &Library,
    hits: Vec<crate::library::storage::AssetSearchHit>,
) -> Result<Vec<SearchResultAssetEntry>, String> {
    let asset_ids = hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>();
    let asset_preview_paths = library.asset_search_preview_paths_for_assets(&asset_ids);
    let active_asset_paths = library.asset_original_paths_for_assets(&asset_ids)?;

    let mut groups = Vec::new();
    for hit in hits {
        if !active_asset_paths.contains_key(&hit.asset_id) {
            continue;
        }
        let asset = library.load_asset(hit.asset_id)?;
        let group = SearchResultAssetEntry {
            asset_id: hit.asset_id,
            asset_preview_path: asset_preview_paths.get(&hit.asset_id).cloned(),
            asset_width: asset.width,
            asset_height: asset.height,
        };
        groups.push(group);
    }

    Ok(groups)
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
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
    canvas_image_max_side: u32,
    asset_id: i64,
    original_path: &PathBuf,
    helper: &mut Option<EmbeddingHelperClient>,
    report_import_embedding_start: bool,
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
    if helper_will_start {
        if let Some(timing) = ensure_embedding_helper(helper)? {
            let _ = result_tx.send(IndexResult::ModelLoadTiming { timing });
        }
        let _ = result_tx.send(IndexResult::ModelReady);
    } else {
        let _ = ensure_embedding_helper(helper)?;
    }
    if report_import_embedding_start {
        let _ = result_tx.send(IndexResult::ImportEmbeddingStarted { asset_id });
    }
    let embedding = embed_image_in_helper(&source_path, helper)?;
    library.upsert_asset_embedding_blob(asset_id, model_spec.model_key, &embedding)?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct EmbedImageHelperOutput {
    embedding: Option<Vec<f32>>,
    error: Option<String>,
    model_load_timing: Option<EmbedModelLoadTimingOutput>,
}

struct EmbeddingHelperClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl EmbeddingHelperClient {
    fn new() -> Result<(Self, ModelLoadTiming), String> {
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
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        let timing = client.read_ready_timing()?;
        Ok((client, timing))
    }

    fn read_ready_timing(&mut self) -> Result<ModelLoadTiming, String> {
        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read embedding helper ready output: {error}"))?;
        if bytes == 0 {
            let status = self.child.try_wait().ok().flatten();
            return Err(match status {
                Some(status) => {
                    format!("embedding helper exited before ready output with {status}")
                }
                None => "embedding helper closed stdout before ready output".to_owned(),
            });
        }
        let output: EmbedImageHelperOutput = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper ready output: {error}"))?;
        if let Some(error) = output.error {
            return Err(error);
        }
        output
            .model_load_timing
            .map(Into::into)
            .ok_or_else(|| "embedding helper ready output missing model timing".to_owned())
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
        let (client, _) = EmbeddingHelperClient::new()?;
        *helper = Some(client);
    }
    helper
        .as_mut()
        .ok_or_else(|| "embedding helper unavailable".to_owned())?
        .embed_image(source_path)
}

fn ensure_embedding_helper(
    helper: &mut Option<EmbeddingHelperClient>,
) -> Result<Option<ModelLoadTiming>, String> {
    if helper.is_none() {
        let (client, timing) = EmbeddingHelperClient::new()?;
        *helper = Some(client);
        Ok(Some(timing))
    } else {
        Ok(None)
    }
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

/// Embedding Helper Grace Period: how long the embedding worker keeps the
/// **Embedding Helper Process** alive after a session's last request before
/// dropping it. Bridges trickled imports (e.g. one drag every couple of
/// seconds) so the model does not cold-mount per asset, while still letting
/// the helper exit after a quiet window so **Quiescent Memory** returns to
/// baseline.
///
/// Default 3s; override with `SNAPPYREF_EMBEDDING_HELPER_GRACE_MS=<ms>`. A
/// value of `0` reproduces the previous behavior (drop helper as soon as the
/// queue drains).
fn embedding_helper_grace() -> Duration {
    const DEFAULT_GRACE_MS: u64 = 3000;
    std::env::var("SNAPPYREF_EMBEDDING_HELPER_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(DEFAULT_GRACE_MS))
}

/// Drive the embedding index worker: per `recv()`, start a fresh helper-owning
/// session, process the first request, then keep accepting requests within
/// the grace window using `recv_timeout`. When grace expires the helper is
/// dropped (returning model and allocator memory to the OS) and the loop
/// blocks on `recv()` for the next session.
///
/// Generic over the request and helper types so the lifetime/loop logic can
/// be tested without spawning a real **Embedding Helper Process**.
fn run_index_worker_loop<T, H, F>(rx: &mpsc::Receiver<T>, grace: Duration, mut process: F)
where
    F: FnMut(T, &mut Option<H>),
{
    while let Ok(first_request) = rx.recv() {
        let mut helper: Option<H> = None;
        process(first_request, &mut helper);
        loop {
            match rx.recv_timeout(grace) {
                Ok(request) => process(request, &mut helper),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        // `helper` drops here at the end of each session, so the
        // **Embedding Helper Process** exits and its native model and
        // allocator memory return to the OS.
    }
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
        IndexResult, SemanticSearchConfig, SemanticSearchService, embedding_helper_grace,
        embedding_source_path, is_terminal_embedding_failure, run_index_worker_loop,
    };
    use crate::library::storage::Library;
    use crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL;
    use crate::search::semantic::model::ModelSpec;
    use eframe::egui;
    use image::imageops::FilterType;
    use std::fs;
    use std::process::Command;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn image_pivot_returns_unplaced_asset_without_creating_placement() {
        let root = temp_root("snappyref-library-service-test");
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
        library.archive_placements(&[unplaced.state.id]).unwrap();
        library
            .upsert_asset_embedding_blob(
                source.require_loaded_asset().id,
                "test-model",
                &[1.0, 0.0],
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(
                unplaced.require_loaded_asset().id,
                "test-model",
                &[0.9, 0.1],
            )
            .unwrap();
        let unplaced_preview_path = library
            .store_canvas_image_cache(
                unplaced.require_loaded_asset().id,
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
            .search_with_image_pivot_with_limit(source.require_loaded_asset().id, None, 5)
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].asset_id, unplaced.require_loaded_asset().id);
        assert_eq!(
            results[0].asset_preview_path.as_deref(),
            Some(unplaced_preview_path.as_path())
        );
        assert_eq!(library.load_placements().unwrap().len(), 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn image_pivot_searches_assets_across_boards() {
        let root = temp_root("snappyref-library-wide-service-test");
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
            .upsert_asset_embedding_blob(
                source.require_loaded_asset().id,
                "test-model",
                &[1.0, 0.0],
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(
                active.require_loaded_asset().id,
                "test-model",
                &[0.8, 0.2],
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(
                inactive.require_loaded_asset().id,
                "test-model",
                &[0.99, 0.01],
            )
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
            .search_with_image_pivot_with_limit(source.require_loaded_asset().id, None, 5)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].asset_id, inactive.require_loaded_asset().id);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn image_pivot_search_tolerates_archived_asset_from_warmed_cache() {
        let root = temp_root("snappyref-archived-cache-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let result_path = library
            .store_rgba_image("result", &[0, 255, 0, 255], 1, 1)
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
        let result = library
            .persist_imported_image(
                &result_path,
                "image/png",
                1,
                1,
                4,
                egui::pos2(10.0, 0.0),
                1.0,
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(
                source.require_loaded_asset().id,
                "test-model",
                &[1.0, 0.0],
            )
            .unwrap();
        library
            .upsert_asset_embedding_blob(
                result.require_loaded_asset().id,
                "test-model",
                &[0.9, 0.1],
            )
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

        assert_eq!(
            service
                .search_with_image_pivot_with_limit(source.require_loaded_asset().id, None, 5)
                .unwrap()[0]
                .asset_id,
            result.require_loaded_asset().id
        );

        library
            .archive_assets(&[result.require_loaded_asset().id])
            .unwrap();

        let results = service
            .search_with_image_pivot_with_limit(source.require_loaded_asset().id, None, 5)
            .unwrap();
        assert!(results.is_empty());

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
            .upsert_asset_embedding_blob(
                record.require_loaded_asset().id,
                "test-model",
                &[1.0, 0.0],
            )
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
            .enqueue_asset_index(record.require_loaded_asset().id, source_path)
            .unwrap();

        assert!(service.try_recv_all().is_empty());
        let embedding = library
            .load_asset_embedding(record.require_loaded_asset().id, "test-model")
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::search::semantic::index::decode_f32_blob(&embedding).unwrap(),
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
            record.require_loaded_asset().id,
            &record.require_loaded_asset().original_path,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        );
        assert_eq!(without_lod, record.require_loaded_asset().original_path);

        let working_lod = library
            .store_canvas_image_cache(
                record.require_loaded_asset().id,
                crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
                &[0, 255, 0, 255],
                1,
                1,
            )
            .unwrap()
            .unwrap();

        let with_lod = embedding_source_path(
            &library,
            record.require_loaded_asset().id,
            &record.require_loaded_asset().original_path,
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
            .enqueue_asset_index(record.require_loaded_asset().id, stored_path.clone())
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
                    IndexResult::ImportEmbeddingStarted { .. } => {}
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
        assert_eq!(results[0].asset_id, record.require_loaded_asset().id);

        let _ = fs::remove_dir_all(root);
    }

    /// A test stand-in for the **Embedding Helper Process** child. The
    /// production helper is an OS subprocess that owns the loaded image
    /// semantic model; here we just need something whose Drop event the
    /// test can observe so we can assert when the worker session ends.
    struct TestHelper {
        sessions_ended: Arc<AtomicUsize>,
    }

    impl Drop for TestHelper {
        fn drop(&mut self) {
            self.sessions_ended.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Run `run_index_worker_loop` on a background thread and collect, in
    /// order, every helper-start, request, and helper-drop event. Used by
    /// the grace-period tests below to assert when the worker keeps a
    /// single session alive vs. starts fresh ones.
    fn run_worker_collecting_events(
        grace: Duration,
    ) -> (
        mpsc::Sender<u32>,
        Arc<Mutex<Vec<String>>>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        thread::JoinHandle<()>,
    ) {
        let (tx, rx) = mpsc::channel::<u32>();
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let helper_starts = Arc::new(AtomicUsize::new(0));
        let helper_ends = Arc::new(AtomicUsize::new(0));

        let events_thread = events.clone();
        let starts_thread = helper_starts.clone();
        let ends_thread = helper_ends.clone();
        let handle = thread::spawn(move || {
            run_index_worker_loop::<u32, TestHelper, _>(&rx, grace, |request, helper| {
                if helper.is_none() {
                    starts_thread.fetch_add(1, Ordering::SeqCst);
                    *helper = Some(TestHelper {
                        sessions_ended: ends_thread.clone(),
                    });
                    events_thread
                        .lock()
                        .unwrap()
                        .push("helper_started".to_owned());
                }
                events_thread
                    .lock()
                    .unwrap()
                    .push(format!("processed:{request}"));
            });
        });

        (tx, events, helper_starts, helper_ends, handle)
    }

    /// Default and env-override behaviour live in one test because
    /// `std::env::set_var` mutates process-global state, and Cargo runs
    /// unit tests in parallel by default — splitting the two cases lets
    /// them race against each other on the same env name.
    #[test]
    fn embedding_helper_grace_default_and_env_override() {
        // We do not assume the absolute default value here in case the
        // ambient test environment sets the variable; instead the env
        // override path is the contract this test pins down.
        unsafe { std::env::set_var("SNAPPYREF_EMBEDDING_HELPER_GRACE_MS", "750") };
        let with_override = embedding_helper_grace();
        unsafe { std::env::remove_var("SNAPPYREF_EMBEDDING_HELPER_GRACE_MS") };
        let without_override = embedding_helper_grace();

        assert_eq!(with_override, Duration::from_millis(750));
        assert_eq!(
            without_override,
            Duration::from_millis(3000),
            "default grace must be 3s when the override env var is unset"
        );
    }

    /// The headline behavior the issue asks us to fix: trickled requests
    /// arriving within the grace window must reuse a single helper
    /// session instead of cold-mounting a fresh model per request.
    ///
    /// Pre-fix this test failed because the worker's inner `try_recv`
    /// drain loop ended the session the instant the queue emptied — even
    /// when the next request arrived only 100ms later.
    #[test]
    fn helper_survives_trickled_requests_within_grace() {
        let grace = Duration::from_millis(500);
        let (tx, events, starts, ends, handle) = run_worker_collecting_events(grace);

        // Send three messages spaced well inside the grace window. With
        // the fix the worker keeps one helper alive across all three.
        tx.send(1).unwrap();
        thread::sleep(Duration::from_millis(120));
        tx.send(2).unwrap();
        thread::sleep(Duration::from_millis(120));
        tx.send(3).unwrap();

        // Drop the sender so the worker thread observes Disconnected
        // after the grace timeout elapses and the loop can finish.
        drop(tx);
        handle.join().unwrap();

        assert_eq!(
            starts.load(Ordering::SeqCst),
            1,
            "expected the helper to start exactly once across trickled requests"
        );
        assert_eq!(
            ends.load(Ordering::SeqCst),
            1,
            "expected the helper to be dropped once after the channel disconnects"
        );

        let events = events.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![
                "helper_started".to_owned(),
                "processed:1".to_owned(),
                "processed:2".to_owned(),
                "processed:3".to_owned(),
            ]
        );
    }

    /// A request that arrives after the grace window expires must start
    /// a fresh helper. This protects the **Quiescent Memory** target:
    /// without it, an indefinitely-running helper would keep the
    /// semantic model resident across long idle periods.
    #[test]
    fn helper_restarts_after_grace_expires() {
        let grace = Duration::from_millis(150);
        let (tx, _events, starts, ends, handle) = run_worker_collecting_events(grace);

        tx.send(1).unwrap();
        // Wait well past grace so the first session ends and the helper
        // is dropped before the next request arrives.
        thread::sleep(Duration::from_millis(450));
        tx.send(2).unwrap();
        thread::sleep(Duration::from_millis(50));
        drop(tx);
        handle.join().unwrap();

        assert_eq!(
            starts.load(Ordering::SeqCst),
            2,
            "expected a fresh helper for the second request after grace expired"
        );
        assert_eq!(
            ends.load(Ordering::SeqCst),
            2,
            "expected both helpers to be dropped once their sessions ended"
        );
    }
}
