use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hf_hub::api::sync::Api;
use image::imageops::FilterType;
use ndarray::{Array2, Array4};
use ort::{session::Session, value::TensorRef};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

pub trait EmbeddingModel: Send + Sync {
    fn embed_text(&self, query: &str) -> Result<Vec<f32>, String>;
    fn embed_image_path(&self, path: &Path) -> Result<Vec<f32>, String>;
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum ResizeStrategy {
    ShortestEdgeCenterCrop,
    Exact,
}

#[derive(Clone, Debug)]
pub struct ModelSpec {
    pub model_key: &'static str,
    pub dimension: usize,
    pub input_size: u32,
    pub repo_id: &'static str,
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
    pub resize_strategy: ResizeStrategy,
}

pub const SIGLIP2_BASE_PATCH16_224_INT8: ModelSpec = ModelSpec {
    model_key: "siglip2_base_patch16_224_int8",
    dimension: 768,
    input_size: 224,
    repo_id: "onnx-community/siglip2-base-patch16-224-ONNX",
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
    resize_strategy: ResizeStrategy::Exact,
};

#[allow(dead_code)]
pub const AVAILABLE_SEMANTIC_SEARCH_MODELS: &[ModelSpec] = &[SIGLIP2_BASE_PATCH16_224_INT8];

pub const DEFAULT_SEMANTIC_SEARCH_MODEL: ModelSpec = SIGLIP2_BASE_PATCH16_224_INT8;

pub(crate) struct ResolvedModelAssets {
    tokenizer_path: PathBuf,
    text_model_path: PathBuf,
    vision_model_path: PathBuf,
}

pub(crate) enum ModelAssetResolution {
    Packaged(ResolvedModelAssets),
    HuggingFace,
}

pub(crate) fn resolve_packaged_model_assets(
    spec: &ModelSpec,
    app_root: &Path,
) -> Option<ResolvedModelAssets> {
    let model_root = app_root.join("models").join(spec.model_key);
    let assets = ResolvedModelAssets {
        tokenizer_path: model_root.join(spec.tokenizer_file),
        text_model_path: model_root.join(spec.text_model_file),
        vision_model_path: model_root.join(spec.vision_model_file),
    };

    if model_asset_exists(&assets.tokenizer_path)
        && model_asset_exists(&assets.text_model_path)
        && model_asset_exists(&assets.vision_model_path)
    {
        Some(assets)
    } else {
        None
    }
}

fn model_asset_exists(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

pub(crate) fn resolve_model_assets_from_app_root(
    spec: &ModelSpec,
    app_root: &Path,
) -> ModelAssetResolution {
    match resolve_packaged_model_assets(spec, app_root) {
        Some(assets) => ModelAssetResolution::Packaged(assets),
        None => ModelAssetResolution::HuggingFace,
    }
}

fn resolve_model_assets<F>(
    spec: &ModelSpec,
    on_progress: &mut F,
) -> Result<ResolvedModelAssets, String>
where
    F: FnMut(&str),
{
    let app_roots = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(packaged_model_roots_from_executable_dir))
        .unwrap_or_default();

    for app_root in app_roots {
        if let ModelAssetResolution::Packaged(assets) =
            resolve_model_assets_from_app_root(spec, &app_root)
        {
            on_progress("Loading packaged model assets…");
            return Ok(assets);
        }
    }

    download_model_assets(spec, on_progress)
}

fn packaged_model_roots_from_executable_dir(executable_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![executable_dir.to_path_buf()];

    if let Some(resources_root) = macos_app_resources_root(executable_dir) {
        roots.insert(0, resources_root);
    }

    roots
}

fn macos_app_resources_root(executable_dir: &Path) -> Option<PathBuf> {
    let contents_dir = executable_dir.parent()?;
    if executable_dir.file_name()? != "MacOS" || contents_dir.file_name()? != "Contents" {
        return None;
    }

    let app_dir = contents_dir.parent()?;
    if app_dir.extension()? != "app" {
        return None;
    }

    Some(contents_dir.join("Resources"))
}

fn download_model_assets<F>(
    spec: &ModelSpec,
    on_progress: &mut F,
) -> Result<ResolvedModelAssets, String>
where
    F: FnMut(&str),
{
    on_progress("Downloading model assets…");
    let repo = Api::new()
        .map_err(|error| format!("failed to initialize Hugging Face API: {error}"))?
        .model(spec.repo_id.to_owned());

    let tokenizer_path = repo
        .get(spec.tokenizer_file)
        .map_err(|error| format!("failed to fetch tokenizer: {error}"))?;
    let text_model_path = repo
        .get(spec.text_model_file)
        .map_err(|error| format!("failed to fetch text model: {error}"))?;
    let vision_model_path = repo
        .get(spec.vision_model_file)
        .map_err(|error| format!("failed to fetch vision model: {error}"))?;

    Ok(ResolvedModelAssets {
        tokenizer_path,
        text_model_path,
        vision_model_path,
    })
}

pub struct OrtDualEncoderModel {
    spec: ModelSpec,
    tokenizer: Tokenizer,
    text_session: Mutex<Session>,
    vision_session: Mutex<Session>,
}

impl OrtDualEncoderModel {
    pub fn load_with_progress<F>(spec: ModelSpec, mut on_progress: F) -> Result<Self, String>
    where
        F: FnMut(&str),
    {
        let assets = resolve_model_assets(&spec, &mut on_progress)?;

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

        on_progress("Initializing model…");
        let text_session = Session::builder()
            .map_err(|error| format!("failed to create text session builder: {error}"))?
            .commit_from_file(&assets.text_model_path)
            .map_err(|error| format!("failed to load text model: {error}"))?;
        let vision_session = Session::builder()
            .map_err(|error| format!("failed to create vision session builder: {error}"))?
            .commit_from_file(&assets.vision_model_path)
            .map_err(|error| format!("failed to load vision model: {error}"))?;

        Ok(Self {
            spec,
            tokenizer,
            text_session: Mutex::new(text_session),
            vision_session: Mutex::new(vision_session),
        })
    }
}

impl EmbeddingModel for OrtDualEncoderModel {
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

    fn embed_image_path(&self, path: &Path) -> Result<Vec<f32>, String> {
        let input = preprocess_image(path, &self.spec)?;
        let mut vision_session = self
            .vision_session
            .lock()
            .map_err(|_| "vision session lock poisoned".to_owned())?;
        let outputs = vision_session
            .run(ort::inputs! {
                "pixel_values" => TensorRef::from_array_view(&input)
                    .map_err(|error| format!("failed to bind image input: {error}"))?
            })
            .map_err(|error| format!("failed to run vision model: {error}"))?;

        let array = outputs[self.spec.vision_output_name]
            .try_extract_array::<f32>()
            .map_err(|error| format!("failed to extract image embedding: {error}"))?;
        Ok(normalize(array.iter().copied().collect()))
    }
}

fn preprocess_image(path: &Path, spec: &ModelSpec) -> Result<Array4<f32>, String> {
    let image = image::open(path)
        .map_err(|error| format!("failed to open image {}: {error}", path.display()))?
        .to_rgb8();
    let (width, height) = image.dimensions();
    let shortest_edge = width.min(height);
    if shortest_edge == 0 {
        return Err(format!(
            "image {} has zero-sized dimensions",
            path.display()
        ));
    }

    let cropped = match spec.resize_strategy {
        ResizeStrategy::ShortestEdgeCenterCrop => {
            let scale = spec.input_size as f32 / shortest_edge as f32;
            let resized_width = ((width as f32 * scale).round() as u32).max(spec.input_size);
            let resized_height = ((height as f32 * scale).round() as u32).max(spec.input_size);
            let resized =
                image::imageops::resize(&image, resized_width, resized_height, spec.resize_filter);
            let crop_x = (resized.width().saturating_sub(spec.input_size)) / 2;
            let crop_y = (resized.height().saturating_sub(spec.input_size)) / 2;
            image::imageops::crop_imm(&resized, crop_x, crop_y, spec.input_size, spec.input_size)
                .to_image()
        }
        ResizeStrategy::Exact => {
            image::imageops::resize(&image, spec.input_size, spec.input_size, spec.resize_filter)
        }
    };

    let size = spec.input_size as usize;
    let mut array = Array4::<f32>::zeros((1, 3, size, size));
    for (x, y, pixel) in cropped.enumerate_pixels() {
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

    Ok(array)
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
    use super::{
        DEFAULT_SEMANTIC_SEARCH_MODEL, ModelAssetResolution,
        packaged_model_roots_from_executable_dir, resolve_model_assets_from_app_root,
        resolve_packaged_model_assets,
    };

    #[test]
    fn resolves_packaged_model_assets_when_all_files_exist() {
        let root =
            std::env::temp_dir().join(format!("snappyref-model-assets-{}", std::process::id()));
        let model_root = root
            .join("models")
            .join(DEFAULT_SEMANTIC_SEARCH_MODEL.model_key);
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

        let assets = resolve_packaged_model_assets(&DEFAULT_SEMANTIC_SEARCH_MODEL, &root).unwrap();

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

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn falls_back_to_hugging_face_when_packaged_assets_are_incomplete() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-model-assets-missing-{}",
            std::process::id()
        ));
        let model_root = root
            .join("models")
            .join(DEFAULT_SEMANTIC_SEARCH_MODEL.model_key);
        std::fs::create_dir_all(model_root.join("onnx")).unwrap();
        std::fs::write(
            model_root.join(DEFAULT_SEMANTIC_SEARCH_MODEL.tokenizer_file),
            "{}",
        )
        .unwrap();

        let resolution = resolve_model_assets_from_app_root(&DEFAULT_SEMANTIC_SEARCH_MODEL, &root);

        assert!(matches!(resolution, ModelAssetResolution::HuggingFace));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn app_bundle_model_roots_prefer_resources() {
        let executable_dir = std::path::Path::new("/fake/SnappyRef/SnappyRef.app/Contents/MacOS");

        let roots = packaged_model_roots_from_executable_dir(executable_dir);

        assert_eq!(
            roots,
            vec![
                std::path::PathBuf::from("/fake/SnappyRef/SnappyRef.app/Contents/Resources"),
                std::path::PathBuf::from("/fake/SnappyRef/SnappyRef.app/Contents/MacOS"),
            ]
        );
    }
}
