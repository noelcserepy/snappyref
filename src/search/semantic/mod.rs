mod index;
mod install;
mod install_controller;
mod model;
mod model_store;
mod service;

pub(crate) use install::SemanticSearchInstallProgress;
#[cfg(not(test))]
pub(crate) use install::remove_semantic_search_assets;
pub(crate) use install::{default_semantic_search_asset_descriptor, install_semantic_search_asset};
pub(crate) use install_controller::{
    SemanticSearchInstallController, SemanticSearchInstallDisplay, SemanticSearchInstallSnapshot,
};
pub use model::DEFAULT_SEMANTIC_SEARCH_MODEL;
pub(crate) use model::{
    ImageEmbeddingTiming, ModelLoadTiming, OrtDualEncoderModel, default_local_model_store_status,
};
pub(crate) use model_store::LocalModelStoreStatus;
pub use service::{
    IndexResult, SearchQueryResult, SearchResultAssetEntry, SemanticSearchConfig,
    SemanticSearchService,
};

#[derive(serde::Serialize)]
struct EmbedImageOutput {
    embedding: Option<Vec<f32>>,
    error: Option<String>,
    model_load_timing: Option<EmbedModelLoadTimingOutput>,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub(crate) struct EmbedModelLoadTimingOutput {
    assets_resolved_ms: f64,
    tokenizer_loaded_ms: f64,
    text_session_loaded_ms: f64,
    vision_session_loaded_ms: f64,
    total_ms: f64,
}

impl From<ModelLoadTiming> for EmbedModelLoadTimingOutput {
    fn from(timing: ModelLoadTiming) -> Self {
        Self {
            assets_resolved_ms: timing.assets_resolved.as_secs_f64() * 1000.0,
            tokenizer_loaded_ms: timing.tokenizer_loaded.as_secs_f64() * 1000.0,
            text_session_loaded_ms: timing.text_session_loaded.as_secs_f64() * 1000.0,
            vision_session_loaded_ms: timing.vision_session_loaded.as_secs_f64() * 1000.0,
            total_ms: timing.total.as_secs_f64() * 1000.0,
        }
    }
}

impl From<EmbedModelLoadTimingOutput> for ModelLoadTiming {
    fn from(timing: EmbedModelLoadTimingOutput) -> Self {
        Self {
            assets_resolved: std::time::Duration::from_secs_f64(timing.assets_resolved_ms / 1000.0),
            tokenizer_loaded: std::time::Duration::from_secs_f64(
                timing.tokenizer_loaded_ms / 1000.0,
            ),
            text_session_loaded: std::time::Duration::from_secs_f64(
                timing.text_session_loaded_ms / 1000.0,
            ),
            vision_session_loaded: std::time::Duration::from_secs_f64(
                timing.vision_session_loaded_ms / 1000.0,
            ),
            total: std::time::Duration::from_secs_f64(timing.total_ms / 1000.0),
        }
    }
}

pub(crate) fn print_embed_image_output(path: &std::path::Path) -> Result<(), String> {
    let (model, _) = model::OrtImageEmbeddingModel::load_with_progress_timed(
        DEFAULT_SEMANTIC_SEARCH_MODEL,
        |_| {},
    )
    .map_err(|error| format!("failed to load semantic model in embedding helper: {error}"))?;
    let embedding = model
        .embed_image_path_timed(path)
        .map(|(embedding, _timing)| embedding)
        .map_err(|error| format!("failed to embed image in embedding helper: {error}"))?;
    serde_json::to_writer(
        std::io::stdout(),
        &EmbedImageOutput {
            embedding: Some(embedding),
            error: None,
            model_load_timing: None,
        },
    )
    .map_err(|error| format!("failed to write embedding helper output: {error}"))?;
    Ok(())
}

pub(crate) fn run_embed_image_server() -> Result<(), String> {
    use std::io::{BufRead, Write};

    let (model, timing) = model::OrtImageEmbeddingModel::load_with_progress_timed(
        DEFAULT_SEMANTIC_SEARCH_MODEL,
        |_| {},
    )
    .map_err(|error| format!("failed to load semantic model in embedding helper: {error}"))?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(
        &mut stdout,
        &EmbedImageOutput {
            embedding: None,
            error: None,
            model_load_timing: Some(timing.into()),
        },
    )
    .map_err(|error| format!("failed to write embedding helper ready output: {error}"))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish embedding helper ready output: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush embedding helper ready output: {error}"))?;
    for line in stdin.lock().lines() {
        let line =
            line.map_err(|error| format!("failed to read embedding helper input: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let path: std::path::PathBuf = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper input: {error}"))?;
        let output = match model.embed_image_path_timed(&path) {
            Ok((embedding, _timing)) => EmbedImageOutput {
                embedding: Some(embedding),
                error: None,
                model_load_timing: None,
            },
            Err(error) => EmbedImageOutput {
                embedding: None,
                error: Some(format!(
                    "failed to embed image in embedding helper: {error}"
                )),
                model_load_timing: None,
            },
        };
        serde_json::to_writer(&mut stdout, &output)
            .map_err(|error| format!("failed to write embedding helper output: {error}"))?;
        stdout
            .write_all(b"\n")
            .map_err(|error| format!("failed to finish embedding helper output: {error}"))?;
        stdout
            .flush()
            .map_err(|error| format!("failed to flush embedding helper output: {error}"))?;
    }
    Ok(())
}
