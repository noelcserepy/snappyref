mod index;
mod model;
mod service;

pub use index::SearchScope;
#[allow(unused_imports)]
pub use model::AVAILABLE_SEMANTIC_SEARCH_MODELS;
pub use model::DEFAULT_SEMANTIC_SEARCH_MODEL;
pub(crate) use model::{ImageEmbeddingTiming, OrtDualEncoderModel};
pub use service::{
    IndexResult, SearchQueryResult, SearchResultGroup, SemanticSearchConfig, SemanticSearchService,
};

#[derive(serde::Serialize)]
struct EmbedImageOutput {
    embedding: Option<Vec<f32>>,
    error: Option<String>,
}

pub(crate) fn print_embed_image_output(path: &std::path::Path) -> Result<(), String> {
    use model::EmbeddingModel;

    let model = OrtDualEncoderModel::load_with_progress(DEFAULT_SEMANTIC_SEARCH_MODEL, |_| {})
        .map_err(|error| format!("failed to load semantic model in embedding helper: {error}"))?;
    let embedding = model
        .embed_image_path(path)
        .map_err(|error| format!("failed to embed image in embedding helper: {error}"))?;
    serde_json::to_writer(
        std::io::stdout(),
        &EmbedImageOutput {
            embedding: Some(embedding),
            error: None,
        },
    )
    .map_err(|error| format!("failed to write embedding helper output: {error}"))?;
    Ok(())
}

pub(crate) fn run_embed_image_server() -> Result<(), String> {
    use std::io::{BufRead, Write};

    use model::EmbeddingModel;

    let model = OrtDualEncoderModel::load_with_progress(DEFAULT_SEMANTIC_SEARCH_MODEL, |_| {})
        .map_err(|error| format!("failed to load semantic model in embedding helper: {error}"))?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line =
            line.map_err(|error| format!("failed to read embedding helper input: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let path: std::path::PathBuf = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper input: {error}"))?;
        let output = match model.embed_image_path(&path) {
            Ok(embedding) => EmbedImageOutput {
                embedding: Some(embedding),
                error: None,
            },
            Err(error) => EmbedImageOutput {
                embedding: None,
                error: Some(format!(
                    "failed to embed image in embedding helper: {error}"
                )),
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
