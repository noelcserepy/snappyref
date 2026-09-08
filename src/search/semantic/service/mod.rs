mod helper;
pub(crate) use helper::run_embed_image_server;
use helper::{EmbeddingHelperClient, embed_image_in_helper, ensure_embedding_helper};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::library::storage::{EmbeddingStatus, Library};

mod index;
use super::model::{EmbeddingModel, ModelLoadTiming, ModelSpec, OrtTextEncoderModel};
use index::AssetIndex;

const IMAGE_PIVOT_CANDIDATE_LIMIT: usize = 50;

pub struct SemanticSearchConfig {
    pub model: ModelSpec,
    pub canvas_image_max_side: u32,
}

#[derive(Clone, Debug)]
pub struct SearchResultAssetEntry {
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

#[derive(Debug, PartialEq, Eq)]
pub enum SearchQueryError {
    ModelPending,
    Failed(String),
}

impl From<String> for SearchQueryError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

pub enum SearchQueryResult {
    ModelPending {
        request_id: u64,
    },
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
    Ready(Arc<OrtTextEncoderModel>),
    Failed(String),
}

pub struct SemanticSearchService {
    #[cfg(test)]
    library: Library,
    model_state: Arc<Mutex<ModelState>>,
    text_embedding_cache: Arc<Mutex<HashMap<String, Vec<f32>>>>,
    asset_index: AssetIndex,
    index_tx: mpsc::Sender<IndexRequest>,
    result_tx: mpsc::Sender<IndexResult>,
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
        let asset_index = AssetIndex::new(library.clone(), config.model.model_key);
        let worker_result_tx = result_tx.clone();
        let worker_library = library.clone();
        let worker_state = model_state.clone();
        let worker_index = asset_index.clone();
        let model_spec = config.model.clone();
        let canvas_image_max_side = config.canvas_image_max_side;

        thread::spawn(move || {
            let grace = embedding_helper_grace();
            run_index_worker_loop(&index_rx, grace, |request, helper| {
                process_index_request(
                    request,
                    &worker_library,
                    &worker_state,
                    &model_spec,
                    &worker_result_tx,
                    &worker_index,
                    canvas_image_max_side,
                    helper,
                );
            });
        });
        let search_library = library.clone();
        let search_state = model_state.clone();
        let search_index_tx = index_tx.clone();
        let search_text_embedding_cache = text_embedding_cache.clone();
        let worker_index = asset_index.clone();
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
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_index,
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
                            &search_state,
                            &search_index_tx,
                            &search_text_embedding_cache,
                            &worker_index,
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
                    Err(SearchQueryError::ModelPending) => {
                        search_result_tx.send(SearchQueryResult::ModelPending { request_id })
                    }
                    Err(SearchQueryError::Failed(error)) => {
                        search_result_tx.send(SearchQueryResult::Failed { request_id, error })
                    }
                };
            }
        });

        let service = Self {
            #[cfg(test)]
            library,
            model_state,
            text_embedding_cache,
            asset_index,
            index_tx,
            result_tx,
            result_rx,
            search_tx,
            search_result_rx,
        };
        Ok(service)
    }

    pub fn enqueue_asset_index(&self, asset_id: i64, original_path: PathBuf) -> Result<(), String> {
        if self.asset_index.contains(asset_id)? {
            // Every accepted request completes, including cache hits. The
            // controller otherwise keeps this asset pending indefinitely.
            return self
                .result_tx
                .send(IndexResult::Indexed { asset_id })
                .map_err(|error| format!("failed to report cached asset indexing: {error}"));
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
    ) -> Result<Vec<SearchResultAssetEntry>, SearchQueryError> {
        search_with_image_pivot_with_limit_for(
            &self.library,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.asset_index,
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
    ) -> Result<Vec<SearchResultAssetEntry>, SearchQueryError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        search_with_limit_for(
            &self.library,
            &self.model_state,
            &self.index_tx,
            &self.text_embedding_cache,
            &self.asset_index,
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
        self.asset_index.is_warming()
    }

    pub fn invalidate_search_embedding_cache(&self) {
        self.asset_index.invalidate();
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
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    asset_index: &AssetIndex,
    source_asset_id: i64,
    text_query: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchResultAssetEntry>, SearchQueryError> {
    let source_embedding = asset_index.pivot_embedding(source_asset_id)?;
    let mut hits = asset_index.search(&source_embedding, IMAGE_PIVOT_CANDIDATE_LIMIT)?;
    hits.retain(|hit| hit.asset_id != source_asset_id);

    let text_query = text_query.map(str::trim).filter(|query| !query.is_empty());
    if let Some(query) = text_query {
        let query_embedding = text_embedding(query, model_state, index_tx, text_embedding_cache)?;

        asset_index.rerank(&mut hits, &query_embedding)?;
    }

    hits.truncate(limit);
    let results = group_hits(library, hits)?;
    Ok(results)
}

fn search_with_limit_for(
    library: &Library,
    model_state: &Arc<Mutex<ModelState>>,
    index_tx: &mpsc::Sender<IndexRequest>,
    text_embedding_cache: &Arc<Mutex<HashMap<String, Vec<f32>>>>,
    asset_index: &AssetIndex,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchResultAssetEntry>, SearchQueryError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let query_embedding = text_embedding(query, model_state, index_tx, text_embedding_cache)?;
    let hits = asset_index.search(&query_embedding, limit)?;
    Ok(group_hits(library, hits)?)
}

fn process_index_request(
    request: IndexRequest,
    worker_library: &Library,
    worker_state: &Arc<Mutex<ModelState>>,
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
    worker_index: &AssetIndex,
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
                worker_index.refresh();
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
                    worker_index.refresh();
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
) -> Result<Vec<f32>, SearchQueryError> {
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
        if error == SearchQueryError::ModelPending {
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

fn ready_model(
    model_state: &Arc<Mutex<ModelState>>,
) -> Result<Arc<OrtTextEncoderModel>, SearchQueryError> {
    let state = model_state
        .lock()
        .map_err(|_| "semantic-search model state poisoned".to_owned())?;
    match &*state {
        ModelState::Uninitialized | ModelState::Loading => Err(SearchQueryError::ModelPending),
        ModelState::Ready(model) => Ok(model.clone()),
        ModelState::Failed(error) => Err(SearchQueryError::Failed(error.clone())),
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
) -> Result<(), String> {
    {
        let mut state = model_state
            .lock()
            .map_err(|_| "semantic-search model state poisoned".to_owned())?;
        match &*state {
            ModelState::Ready(_) => return Ok(()),
            ModelState::Failed(error) => return Err(error.clone()),
            ModelState::Uninitialized => *state = ModelState::Loading,
            ModelState::Loading => return Ok(()),
        }
    }

    let _ = result_tx.send(IndexResult::ModelLoadStarted);
    match OrtTextEncoderModel::load_with_progress_timed(model_spec.clone(), |message| {
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
            *state = ModelState::Ready(model);
            let _ = result_tx.send(IndexResult::ModelReady);
            Ok(())
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
    use crate::test_support::TempRoot;
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
    fn pending_model_requests_initialization_without_classifying_failure_text() {
        let (index_tx, index_rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(super::ModelState::Uninitialized));
        let cache = Arc::new(Mutex::new(std::collections::HashMap::new()));
        assert_eq!(
            super::text_embedding("cat", &state, &index_tx, &cache),
            Err(super::SearchQueryError::ModelPending)
        );
        assert!(matches!(index_rx.try_recv(), Ok(super::IndexRequest::Init)));
        *state.lock().unwrap() = super::ModelState::Failed("Loading model…".to_owned());
        assert_eq!(
            super::text_embedding("cat", &state, &index_tx, &cache),
            Err(super::SearchQueryError::Failed("Loading model…".to_owned()))
        );
        assert!(index_rx.try_recv().is_err());
    }

    #[test]
    fn image_pivot_returns_unplaced_asset_without_creating_placement() {
        let root = TempRoot::new("snappyref-library-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let unplaced_path = library
            .store_rgba_image("unplaced", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let source = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&source_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let unplaced = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(10.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&unplaced_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        library.fixture_board_document(library.load_user_session().unwrap().active_board_id, crate::canvas::CanvasDocumentChanges {archived_placement_ids: (&[unplaced.state.id]).to_vec(), stack_order: Some(library.load_board_document(library.load_user_session().unwrap().active_board_id).unwrap().stack_order.into_iter().filter(|item| !matches!(item, crate::data::CanvasStackItem::Placement(id) if (&[unplaced.state.id]).contains(id))).collect()), ..Default::default()}).unwrap();
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
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
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
    }

    #[test]
    fn image_pivot_searches_assets_across_boards() {
        let root = TempRoot::new("snappyref-library-wide-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let second_board = library.fixture_board("Second").unwrap();
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
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&source_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let active = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(10.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&active_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let inactive = library
            .fixture_placement(
                second_board.id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(20.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&inactive_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
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
    }

    #[test]
    fn image_pivot_search_tolerates_archived_asset_from_warmed_cache() {
        let root = TempRoot::new("snappyref-archived-cache-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let result_path = library
            .store_rgba_image("result", &[0, 255, 0, 255], 1, 1)
            .unwrap();

        let source = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&source_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
            )
            .unwrap();
        let result = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(10.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&result_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
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
    }

    #[test]
    fn cached_asset_index_reports_completion_without_loading_model() {
        let root = TempRoot::new("snappyref-ready-index-skip-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let source_path = library
            .store_rgba_image("ready", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&source_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
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
                model: test_model_spec(),
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();

        service
            .enqueue_asset_index(record.require_loaded_asset().id, source_path)
            .unwrap();

        assert!(matches!(
            service.result_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            IndexResult::Indexed { asset_id } if asset_id == record.require_loaded_asset().id
        ));
        assert!(service.try_recv_all().is_empty());
        let embedding = library
            .load_asset_embedding(record.require_loaded_asset().id, "test-model")
            .unwrap()
            .unwrap();
        assert_eq!(
            embedding
                .chunks_exact(4)
                .map(|bytes| f32::from_ne_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>(),
            vec![1.0, 0.0]
        );
    }

    #[test]
    fn idle_backfill_without_jobs_does_not_initialize_model() {
        let root = TempRoot::new("snappyref-empty-backfill-no-model-service-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
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
    }

    #[test]
    fn embedding_source_prefers_working_lod_when_present() {
        let root = TempRoot::new("snappyref-embedding-source-lod-test");
        let library = Library::initialize(root.join("library")).unwrap();
        let original_path = library
            .store_rgba_image("source", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let record = library
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(&original_path, "image/png", 1, 1, 4, None)
                        .unwrap(),
                ),
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
            .unwrap();

        let with_lod = embedding_source_path(
            &library,
            record.require_loaded_asset().id,
            &record.require_loaded_asset().original_path,
            crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
        );
        assert_eq!(with_lod, working_lod);
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
            .fixture_placement(
                library.load_user_session().unwrap().active_board_id,
                crate::data::Placement::loaded(
                    crate::data::PlacementState::new(
                        library.allocate_document_id(),
                        egui::pos2(0.0, 0.0),
                        1.0,
                        0.0,
                        0,
                        false,
                    ),
                    library
                        .register_original(
                            &stored_path,
                            "image/jpeg",
                            width,
                            height,
                            file_size,
                            None,
                        )
                        .unwrap(),
                ),
            )
            .unwrap();

        let service = SemanticSearchService::new(
            library.clone(),
            SemanticSearchConfig {
                model: DEFAULT_SEMANTIC_SEARCH_MODEL,
                canvas_image_max_side: crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE,
            },
        )
        .unwrap();
        service
            .enqueue_asset_index(record.require_loaded_asset().id, stored_path.clone())
            .unwrap();
        // Queries initialize the text model lazily, just as in the app.
        let _ = service.search_with_limit("cat", 5);

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

            if indexed && service.is_ready() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(indexed, "timed out waiting for semantic-search indexing");

        let results = service.search_with_limit("cat", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].asset_id, record.require_loaded_asset().id);
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
