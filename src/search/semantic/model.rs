use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use image::imageops::FilterType;
use ndarray::{Array2, Array4};
use ort::{ep, session::Session, value::TensorRef};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

use super::model_store::{
    LocalModelStoreStatus, MISSING_MODEL_USER_MESSAGE, model_store_root,
    refresh_local_model_store_status,
};

pub trait EmbeddingModel: Send + Sync {
    fn embed_text(&self, query: &str) -> Result<Vec<f32>, String>;
}

#[derive(Clone, Debug)]
pub struct ModelSpec {
    pub model_key: &'static str,
    pub dimension: usize,
    pub input_size: u32,
    pub text_model_file: &'static str,
    pub vision_model_file: &'static str,
    pub tokenizer_file: &'static str,
    pub token_length: usize,
    pub pad_id: u32,
    pub pad_token: &'static str,
    pub text_output_name: &'static str,
    pub vision_output_name: &'static str,
    pub normalize_mean: Option<[f32; 3]>,
    pub normalize_std: Option<[f32; 3]>,
    pub resize_filter: FilterType,
}

pub const SIGLIP2_BASE_PATCH16_224_INT8: ModelSpec = ModelSpec {
    model_key: "siglip2_base_patch16_224_int8",
    dimension: 768,
    input_size: 224,
    text_model_file: "onnx/text_model_int8.onnx",
    vision_model_file: "onnx/vision_model_int8.onnx",
    tokenizer_file: "tokenizer.json",
    token_length: 64,
    pad_id: 0,
    pad_token: "<pad>",
    text_output_name: "pooler_output",
    vision_output_name: "pooler_output",
    normalize_mean: Some([0.5, 0.5, 0.5]),
    normalize_std: Some([0.5, 0.5, 0.5]),
    resize_filter: FilterType::Triangle,
};

pub const DEFAULT_SEMANTIC_SEARCH_MODEL: ModelSpec = SIGLIP2_BASE_PATCH16_224_INT8;

pub(crate) struct ResolvedModelAssets {
    tokenizer_path: PathBuf,
    text_model_path: PathBuf,
    vision_model_path: PathBuf,
}

impl ModelSpec {
    pub(crate) fn required_model_files(&self) -> [&'static str; 3] {
        [
            self.tokenizer_file,
            self.text_model_file,
            self.vision_model_file,
        ]
    }
}

/// Resolve a [`ModelSpec`] inside a Local Model Store rooted at `store_root`.
///
/// Returns `Ok(assets)` when every required file exists and is non-empty,
/// otherwise returns `Err` with a developer-facing description of the first
/// missing piece. Callers that want to surface a user-facing message should
/// inspect [`super::model_store::classify_availability`] before calling this.
pub(crate) fn resolve_assets_in_store(
    spec: &ModelSpec,
    store_root: &Path,
) -> Result<ResolvedModelAssets, String> {
    let model_root = store_root.join(spec.model_key);
    let assets = ResolvedModelAssets {
        tokenizer_path: model_root.join(spec.tokenizer_file),
        text_model_path: model_root.join(spec.text_model_file),
        vision_model_path: model_root.join(spec.vision_model_file),
    };

    if !model_asset_exists(&assets.tokenizer_path) {
        return Err(format!(
            "missing model file in Local Model Store: {}",
            assets.tokenizer_path.display()
        ));
    }
    if !model_asset_exists(&assets.text_model_path) {
        return Err(format!(
            "missing model file in Local Model Store: {}",
            assets.text_model_path.display()
        ));
    }
    if !model_asset_exists(&assets.vision_model_path) {
        return Err(format!(
            "missing model file in Local Model Store: {}",
            assets.vision_model_path.display()
        ));
    }
    Ok(assets)
}

fn model_asset_exists(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn resolve_model_assets<F>(
    spec: &ModelSpec,
    on_progress: &mut F,
) -> Result<ResolvedModelAssets, String>
where
    F: FnMut(&str),
{
    let data_root = crate::library::location::resolve_data_root()?;
    let executable_dir = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_owned())?
        .to_path_buf();

    match local_model_store_status_for(&data_root, &executable_dir, spec) {
        LocalModelStoreStatus::Installed => {}
        LocalModelStoreStatus::InstalledViaMigration => {
            on_progress("Migrated bundled model to Local Model Store…");
        }
        LocalModelStoreStatus::Missing => return Err(MISSING_MODEL_USER_MESSAGE.to_owned()),
        LocalModelStoreStatus::Downloading { .. } => {
            return Err("Semantic Search assets are still installing.".to_owned());
        }
        LocalModelStoreStatus::Failed(error) => return Err(error),
    }

    on_progress("Loading model assets…");
    resolve_assets_in_store(spec, &model_store_root(&data_root))
}

pub(crate) fn default_local_model_store_status() -> LocalModelStoreStatus {
    let data_root = match crate::library::location::resolve_data_root() {
        Ok(root) => root,
        Err(error) => return LocalModelStoreStatus::Failed(error),
    };
    let executable_dir = match std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))
        .and_then(|path| {
            path.parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "executable has no parent directory".to_owned())
        }) {
        Ok(path) => path,
        Err(error) => return LocalModelStoreStatus::Failed(error),
    };
    local_model_store_status_for(&data_root, &executable_dir, &DEFAULT_SEMANTIC_SEARCH_MODEL)
}

fn local_model_store_status_for(
    data_root: &Path,
    executable_dir: &Path,
    spec: &ModelSpec,
) -> LocalModelStoreStatus {
    let required_files = spec.required_model_files();
    refresh_local_model_store_status(data_root, executable_dir, spec.model_key, &required_files)
}

/// In-process text encoder for search queries. Image embedding runs in the
/// `--semantic-embed-server` subprocess via [`OrtImageEmbeddingModel`].
pub struct OrtTextEncoderModel {
    spec: ModelSpec,
    tokenizer: Tokenizer,
    text_session: Mutex<Session>,
}

pub(crate) struct OrtImageEmbeddingModel {
    spec: ModelSpec,
    vision_session: Mutex<Session>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImageEmbeddingTiming {
    pub decode_rgb: Duration,
    pub resize_crop: Duration,
    pub tensor_prepare: Duration,
    pub bind_input: Duration,
    pub model_run: Duration,
    pub extract_output: Duration,
    pub normalize_output: Duration,
    pub total: Duration,
    pub source_width: u32,
    pub source_height: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ModelLoadTiming {
    pub assets_resolved: Duration,
    pub tokenizer_loaded: Duration,
    pub text_session_loaded: Duration,
    pub vision_session_loaded: Duration,
    pub total: Duration,
}

impl OrtTextEncoderModel {
    pub(crate) fn load_with_progress_timed<F>(
        spec: ModelSpec,
        mut on_progress: F,
    ) -> Result<(Self, ModelLoadTiming), String>
    where
        F: FnMut(&str),
    {
        let total_start = Instant::now();
        let assets_start = Instant::now();
        let assets = resolve_model_assets(&spec, &mut on_progress)?;
        let mut timing = ModelLoadTiming {
            assets_resolved: assets_start.elapsed(),
            ..ModelLoadTiming::default()
        };

        let tokenizer_start = Instant::now();
        let mut tokenizer = Tokenizer::from_file(&assets.tokenizer_path)
            .map_err(|error| format!("failed to load tokenizer: {error}"))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: spec.token_length,
                ..TruncationParams::default()
            }))
            .map_err(|error| format!("failed to configure tokenizer truncation: {error}"))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(spec.token_length),
            pad_id: spec.pad_id,
            pad_token: spec.pad_token.to_owned(),
            ..PaddingParams::default()
        }));
        timing.tokenizer_loaded = tokenizer_start.elapsed();

        on_progress("Initializing model…");
        let text_start = Instant::now();
        let text_session = Session::builder()
            .map_err(|error| format!("failed to create text session builder: {error}"))?
            .with_memory_pattern(false)
            .map_err(|error| format!("failed to disable text memory pattern: {error}"))?
            .with_execution_providers([ep::CPU::default().with_arena_allocator(false).build()])
            .map_err(|error| format!("failed to configure text CPU provider: {error}"))?
            .with_intra_threads(1)
            .map_err(|error| format!("failed to configure text intra-op threads: {error}"))?
            .with_inter_threads(1)
            .map_err(|error| format!("failed to configure text inter-op threads: {error}"))?
            .commit_from_file(&assets.text_model_path)
            .map_err(|error| format!("failed to load text model: {error}"))?;
        timing.text_session_loaded = text_start.elapsed();
        timing.total = total_start.elapsed();

        Ok((
            Self {
                spec,
                tokenizer,
                text_session: Mutex::new(text_session),
            },
            timing,
        ))
    }
}

impl OrtImageEmbeddingModel {
    pub(crate) fn load_with_progress_timed<F>(
        spec: ModelSpec,
        mut on_progress: F,
    ) -> Result<(Self, ModelLoadTiming), String>
    where
        F: FnMut(&str),
    {
        let total_start = Instant::now();
        let assets_start = Instant::now();
        let assets = resolve_model_assets(&spec, &mut on_progress)?;
        let mut timing = ModelLoadTiming {
            assets_resolved: assets_start.elapsed(),
            ..ModelLoadTiming::default()
        };

        on_progress("Initializing image embedding model…");
        let vision_start = Instant::now();
        let vision_session = Session::builder()
            .map_err(|error| format!("failed to create vision session builder: {error}"))?
            .with_memory_pattern(false)
            .map_err(|error| format!("failed to disable vision memory pattern: {error}"))?
            .with_execution_providers([ep::CPU::default().with_arena_allocator(false).build()])
            .map_err(|error| format!("failed to configure vision CPU provider: {error}"))?
            .with_intra_threads(1)
            .map_err(|error| format!("failed to configure vision intra-op threads: {error}"))?
            .with_inter_threads(1)
            .map_err(|error| format!("failed to configure vision inter-op threads: {error}"))?
            .commit_from_file(&assets.vision_model_path)
            .map_err(|error| format!("failed to load vision model: {error}"))?;
        timing.vision_session_loaded = vision_start.elapsed();
        timing.total = total_start.elapsed();

        Ok((
            Self {
                spec,
                vision_session: Mutex::new(vision_session),
            },
            timing,
        ))
    }

    pub(crate) fn embed_image_path_timed(
        &self,
        path: &Path,
    ) -> Result<(Vec<f32>, ImageEmbeddingTiming), String> {
        embed_image_path_with_session(&self.spec, &self.vision_session, path)
    }
}

impl EmbeddingModel for OrtTextEncoderModel {
    fn embed_text(&self, query: &str) -> Result<Vec<f32>, String> {
        let encoding = self
            .tokenizer
            .encode(query, true)
            .map_err(|error| format!("failed to tokenize query: {error}"))?;
        let input_ids = Array2::from_shape_vec(
            (1, encoding.get_ids().len()),
            encoding
                .get_ids()
                .iter()
                .map(|id| i64::from(*id))
                .collect::<Vec<_>>(),
        )
        .map_err(|error| format!("failed to build text tensor: {error}"))?;

        let mut text_session = self
            .text_session
            .lock()
            .map_err(|_| "text session lock poisoned".to_owned())?;
        let outputs = text_session
            .run(ort::inputs! {
                "input_ids" => TensorRef::from_array_view(&input_ids)
                    .map_err(|error| format!("failed to bind text input: {error}"))?
            })
            .map_err(|error| format!("failed to run text model: {error}"))?;

        let array = outputs[self.spec.text_output_name]
            .try_extract_array::<f32>()
            .map_err(|error| format!("failed to extract text embedding: {error}"))?;
        Ok(normalize(array.iter().copied().collect()))
    }
}

fn embed_image_path_with_session(
    spec: &ModelSpec,
    vision_session: &Mutex<Session>,
    path: &Path,
) -> Result<(Vec<f32>, ImageEmbeddingTiming), String> {
    let total_start = Instant::now();
    let (input, mut timing) = preprocess_image_timed(path, spec)?;

    let mut vision_session = vision_session
        .lock()
        .map_err(|_| "vision session lock poisoned".to_owned())?;

    let bind_start = Instant::now();
    let pixel_values = TensorRef::from_array_view(&input)
        .map_err(|error| format!("failed to bind image input: {error}"))?;
    timing.bind_input = bind_start.elapsed();

    let run_start = Instant::now();
    let outputs = vision_session
        .run(ort::inputs! {
            "pixel_values" => pixel_values
        })
        .map_err(|error| format!("failed to run vision model: {error}"))?;
    timing.model_run = run_start.elapsed();

    let extract_start = Instant::now();
    let array = outputs[spec.vision_output_name]
        .try_extract_array::<f32>()
        .map_err(|error| format!("failed to extract image embedding: {error}"))?;
    let embedding = array.iter().copied().collect::<Vec<_>>();
    timing.extract_output = extract_start.elapsed();

    let normalize_start = Instant::now();
    let embedding = normalize(embedding);
    timing.normalize_output = normalize_start.elapsed();
    timing.total = total_start.elapsed();

    Ok((embedding, timing))
}

fn preprocess_image_timed(
    path: &Path,
    spec: &ModelSpec,
) -> Result<(Array4<f32>, ImageEmbeddingTiming), String> {
    let mut timing = ImageEmbeddingTiming::default();
    let decode_start = Instant::now();
    let image = image::open(path)
        .map_err(|error| format!("failed to open image {}: {error}", path.display()))?
        .to_rgb8();
    timing.decode_rgb = decode_start.elapsed();
    let (width, height) = image.dimensions();
    timing.source_width = width;
    timing.source_height = height;
    if width == 0 || height == 0 {
        return Err(format!(
            "image {} has zero-sized dimensions",
            path.display()
        ));
    }

    let resize_start = Instant::now();
    let resized =
        image::imageops::resize(&image, spec.input_size, spec.input_size, spec.resize_filter);
    timing.resize_crop = resize_start.elapsed();

    let tensor_start = Instant::now();
    let size = spec.input_size as usize;
    let mut array = Array4::<f32>::zeros((1, 3, size, size));
    for (x, y, pixel) in resized.enumerate_pixels() {
        let [r, g, b] = pixel.0;
        let mut channels = [
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        ];
        if let (Some(mean), Some(std)) = (spec.normalize_mean, spec.normalize_std) {
            for channel_index in 0..3 {
                channels[channel_index] =
                    (channels[channel_index] - mean[channel_index]) / std[channel_index];
            }
        }
        array[[0, 0, y as usize, x as usize]] = channels[0];
        array[[0, 1, y as usize, x as usize]] = channels[1];
        array[[0, 2, y as usize, x as usize]] = channels[2];
    }
    timing.tensor_prepare = tensor_start.elapsed();

    Ok((array, timing))
}

fn normalize(mut values: Vec<f32>) -> Vec<f32> {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in &mut values {
            *value /= norm;
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SEMANTIC_SEARCH_MODEL, resolve_assets_in_store};

    #[test]
    fn resolves_assets_when_all_files_exist_in_local_model_store() {
        let store_root = std::env::temp_dir().join(format!(
            "snappyref-model-store-resolve-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let model_root = store_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.model_key);
        std::fs::create_dir_all(model_root.join("onnx")).unwrap();
        std::fs::write(
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.tokenizer_file),
            "{}",
        )
        .unwrap();
        std::fs::write(
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.text_model_file),
            "model",
        )
        .unwrap();
        std::fs::write(
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.vision_model_file),
            "model",
        )
        .unwrap();

        let assets = resolve_assets_in_store(&DEFAULT_SEMANTIC_SEARCH_MODEL, &store_root).unwrap();

        assert_eq!(
            assets.tokenizer_path,
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.tokenizer_file)
        );
        assert_eq!(
            assets.text_model_path,
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.text_model_file)
        );
        assert_eq!(
            assets.vision_model_path,
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.vision_model_file)
        );

        let _ = std::fs::remove_dir_all(store_root);
    }

    #[test]
    fn errors_when_local_model_store_is_incomplete() {
        let store_root = std::env::temp_dir().join(format!(
            "snappyref-model-store-missing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let model_root = store_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.model_key);
        std::fs::create_dir_all(model_root.join("onnx")).unwrap();
        std::fs::write(
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.tokenizer_file),
            "{}",
        )
        .unwrap();

        let result = resolve_assets_in_store(&DEFAULT_SEMANTIC_SEARCH_MODEL, &store_root);

        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(store_root);
    }
}
