use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use crate::storage::{EmbeddingStatus, Library};

use super::index::{BruteForceIndex, VectorIndex};
use super::model::{EmbeddingModel, ModelSpec, OrtDualEncoderModel};

pub struct SemanticSearchConfig {
    pub enabled: bool,
    pub model: ModelSpec,
    pub max_results: usize,
}

#[derive(Clone, Debug)]
pub struct SearchResultGroup {
    pub asset_id: i64,
    pub similarity: f32,
    pub item_ids: Vec<i64>,
    pub instance_count: usize,
}

enum IndexRequest {
    Init,
    Asset {
        asset_id: i64,
        original_path: PathBuf,
    },
    Backfill,
}

pub enum IndexResult {
    Indexed,
    Failed { error: String },
    Progress { message: String },
    ModelReady,
}

enum ModelState {
    Uninitialized,
    Loading,
    Ready(Arc<OrtDualEncoderModel>),
    Failed(String),
}

pub struct SemanticSearchService {
    library: Library,
    model_key: &'static str,
    max_results: usize,
    model_state: Arc<Mutex<ModelState>>,
    index_tx: mpsc::Sender<IndexRequest>,
    result_rx: mpsc::Receiver<IndexResult>,
}

impl SemanticSearchService {
    pub fn new(library: Library, config: SemanticSearchConfig) -> Result<Self, String> {
        let (index_tx, index_rx) = mpsc::channel::<IndexRequest>();
        let (result_tx, result_rx) = mpsc::channel::<IndexResult>();
        let model_state = Arc::new(Mutex::new(ModelState::Uninitialized));
        let worker_library = library.clone();
        let worker_state = model_state.clone();
        let model_spec = config.model.clone();

        thread::spawn(move || {
            while let Ok(request) = index_rx.recv() {
                match request {
                    IndexRequest::Init => {
                        let _ = ensure_model(&worker_state, &model_spec, &result_tx);
                    }
                    IndexRequest::Asset {
                        asset_id,
                        original_path,
                    } => {
                        if let Err(error) = index_asset(
                            &worker_library,
                            &worker_state,
                            &model_spec,
                            &result_tx,
                            asset_id,
                            &original_path,
                        ) {
                            let _ = worker_library.upsert_asset_embedding_status(
                                asset_id,
                                model_spec.model_key,
                                model_spec.dimension,
                                EmbeddingStatus::Failed,
                                Some(&error),
                            );
                            let _ = result_tx.send(IndexResult::Failed { error });
                        } else {
                            let _ = result_tx.send(IndexResult::Indexed);
                        }
                    }
                    IndexRequest::Backfill => {
                        if let Err(error) = ensure_model(&worker_state, &model_spec, &result_tx) {
                            let _ = result_tx.send(IndexResult::Failed { error });
                            continue;
                        }

                        let jobs = match worker_library
                            .missing_live_canvas_asset_jobs_for_model(model_spec.model_key)
                        {
                            Ok(jobs) => jobs,
                            Err(error) => {
                                let _ = result_tx.send(IndexResult::Failed { error });
                                continue;
                            }
                        };

                        for job in jobs {
                            match index_asset(
                                &worker_library,
                                &worker_state,
                                &model_spec,
                                &result_tx,
                                job.asset_id,
                                &job.original_path,
                            ) {
                                Ok(()) => {
                                    let _ = result_tx.send(IndexResult::Indexed);
                                }
                                Err(error) => {
                                    let _ = worker_library.upsert_asset_embedding_status(
                                        job.asset_id,
                                        model_spec.model_key,
                                        model_spec.dimension,
                                        EmbeddingStatus::Failed,
                                        Some(&error),
                                    );
                                    let _ = result_tx.send(IndexResult::Failed { error });
                                }
                            }
                        }
                    }
                }
            }
        });

        let service = Self {
            library,
            model_key: config.model.model_key,
            max_results: config.max_results,
            model_state,
            index_tx,
            result_rx,
        };
        if config.enabled {
            let _ = service.index_tx.send(IndexRequest::Init);
        }
        Ok(service)
    }

    pub fn enqueue_asset_index(&self, asset_id: i64, original_path: PathBuf) -> Result<(), String> {
        self.index_tx
            .send(IndexRequest::Asset {
                asset_id,
                original_path,
            })
            .map_err(|error| format!("failed to queue asset indexing: {error}"))
    }

    pub fn enqueue_startup_backfill(&self) -> Result<(), String> {
        self.index_tx
            .send(IndexRequest::Backfill)
            .map_err(|error| format!("failed to queue embedding backfill: {error}"))
    }

    pub fn search(&self, query: &str) -> Result<Vec<SearchResultGroup>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }

        let model = match self.ready_model() {
            Ok(model) => model,
            Err(error) => {
                if error == "Loading model…" {
                    let _ = self.index_tx.send(IndexRequest::Init);
                }
                return Err(error);
            }
        };

        let query_embedding = model.embed_text(query)?;
        let index = BruteForceIndex::new(self.library.clone());
        let hits = index.search(self.model_key, &query_embedding, self.max_results)?;
        let asset_ids = hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>();
        let instances = self.library.board_item_instances_for_assets(&asset_ids)?;

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
                    similarity: hit.similarity,
                    instance_count: item_ids.len(),
                    item_ids,
                }
            })
            .filter(|group| !group.item_ids.is_empty())
            .collect())
    }

    pub fn try_recv_all(&self) -> Vec<IndexResult> {
        self.result_rx.try_iter().collect()
    }

    pub fn is_ready(&self) -> bool {
        self.model_state
            .lock()
            .map(|state| matches!(&*state, ModelState::Ready(_)))
            .unwrap_or(false)
    }

    fn ready_model(&self) -> Result<Arc<OrtDualEncoderModel>, String> {
        let state = self
            .model_state
            .lock()
            .map_err(|_| "semantic-search model state poisoned".to_owned())?;
        match &*state {
            ModelState::Uninitialized | ModelState::Loading => Err("Loading model…".to_owned()),
            ModelState::Ready(model) => Ok(model.clone()),
            ModelState::Failed(error) => Err(error.clone()),
        }
    }
}

fn index_asset(
    library: &Library,
    model_state: &Arc<Mutex<ModelState>>,
    model_spec: &ModelSpec,
    result_tx: &mpsc::Sender<IndexResult>,
    asset_id: i64,
    original_path: &PathBuf,
) -> Result<(), String> {
    library.upsert_asset_embedding_status(
        asset_id,
        model_spec.model_key,
        model_spec.dimension,
        EmbeddingStatus::Pending,
        None,
    )?;
    let model = ensure_model(model_state, model_spec, result_tx)?;
    let embedding = model.embed_image_path(original_path)?;
    library.upsert_asset_embedding_blob(asset_id, model_spec.model_key, &embedding)?;
    Ok(())
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

    match OrtDualEncoderModel::load_with_progress(model_spec.clone(), |message| {
        let _ = result_tx.send(IndexResult::Progress {
            message: message.to_owned(),
        });
    }) {
        Ok(model) => {
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
    use super::{IndexResult, SemanticSearchConfig, SemanticSearchService};
    use crate::semantic_search::DEFAULT_SEMANTIC_SEARCH_MODEL;
    use crate::storage::Library;
    use eframe::egui;
    use std::fs;
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    #[ignore = "downloads the active retrieval model and a sample image for live smoke validation"]
    fn live_smoke_indexes_and_queries_single_image() {
        let root = std::env::temp_dir().join(format!(
            "reference-semantic-search-{}",
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
                max_results: 5,
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
                    IndexResult::Indexed => indexed = true,
                    IndexResult::Failed { error } => panic!("{error}"),
                    IndexResult::Progress { .. } => {}
                    IndexResult::ModelReady => {}
                }
            }

            if indexed {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(indexed, "timed out waiting for semantic-search indexing");

        let results = service.search("cat").unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].asset_id, record.asset.id);

        let _ = fs::remove_dir_all(root);
    }
}
