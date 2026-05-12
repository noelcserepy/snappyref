#![cfg_attr(test, allow(dead_code))]

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use zip::ZipArchive;

use crate::model_store::model_store_root;
use crate::semantic_search::DEFAULT_SEMANTIC_SEARCH_MODEL;
use crate::updater::{download, install_paths};

const DEFAULT_SEMANTIC_SEARCH_ASSET_URL: &str = "https://github.com/noelcserepy/snappyref-dist/releases/download/semantic-search/snappyref-semantic-search.zip";

const SEMANTIC_SEARCH_DOWNLOAD_NAME: &str = "semantic-search-download.zip";
const SEMANTIC_SEARCH_STAGING_NAME: &str = "semantic-search-staging";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SemanticSearchAssetDescriptor {
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) model_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SemanticSearchInstallProgress {
    Progress { percent: u8 },
    Installed,
    Failed { error: String },
    Canceled,
}

#[derive(Debug)]
enum SemanticSearchInstallError {
    Descriptor(String),
    Download(String),
    InvalidZip(String),
    Io(io::Error),
    PathTraversal { entry: String },
    InvalidLayout(String),
}

impl std::fmt::Display for SemanticSearchInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Descriptor(error) => write!(f, "{error}"),
            Self::Download(error) => write!(f, "{error}"),
            Self::InvalidZip(error) => write!(f, "corrupt zip: {error}"),
            Self::Io(error) => write!(f, "io: {error}"),
            Self::PathTraversal { entry } => write!(f, "unsafe zip path `{entry}`"),
            Self::InvalidLayout(error) => write!(f, "{error}"),
        }
    }
}

impl From<io::Error> for SemanticSearchInstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn default_semantic_search_asset_descriptor() -> SemanticSearchAssetDescriptor {
    SemanticSearchAssetDescriptor {
        url: option_env!("SNAPPYREF_SEMANTIC_SEARCH_URL")
            .unwrap_or(DEFAULT_SEMANTIC_SEARCH_ASSET_URL)
            .to_owned(),
        sha256: option_env!("SNAPPYREF_SEMANTIC_SEARCH_SHA256")
            .unwrap_or("")
            .to_owned(),
        size: option_env!("SNAPPYREF_SEMANTIC_SEARCH_SIZE")
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(0),
        model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
    }
}

pub(crate) fn cleanup_semantic_search_install(install_root: &Path) {
    let descriptor = default_semantic_search_asset_descriptor();
    cleanup_install_paths(install_root, &descriptor.model_key);
}

pub(crate) fn remove_semantic_search_assets(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
) -> Result<(), String> {
    cleanup_install_paths(install_root, &descriptor.model_key);
    let model_root = model_store_root(install_root).join(&descriptor.model_key);
    if model_root.exists() {
        fs::remove_dir_all(&model_root)
            .map_err(|error| format!("failed to remove {}: {error}", model_root.display()))?;
    }
    Ok(())
}

pub(crate) fn install_semantic_search_asset<F>(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    canceled: &AtomicBool,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(SemanticSearchInstallProgress),
{
    install_inner(install_root, descriptor, canceled, &mut on_progress).map_err(|error| {
        cleanup_install_paths(install_root, &descriptor.model_key);
        let message = error.to_string();
        if canceled.load(Ordering::Relaxed) {
            on_progress(SemanticSearchInstallProgress::Canceled);
        } else {
            on_progress(SemanticSearchInstallProgress::Failed {
                error: message.clone(),
            });
        }
        message
    })
}

fn install_inner(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    canceled: &AtomicBool,
    on_progress: &mut dyn FnMut(SemanticSearchInstallProgress),
) -> Result<(), SemanticSearchInstallError> {
    validate_descriptor(descriptor)?;
    check_canceled(canceled)?;
    cleanup_install_paths(install_root, &descriptor.model_key);
    let updates_dir = install_paths::updates_dir(install_root);
    fs::create_dir_all(&updates_dir)?;
    let zip_path = updates_dir.join(SEMANTIC_SEARCH_DOWNLOAD_NAME);
    let mut progress = |written: u64, total: u64| {
        let percent = if total == 0 {
            0
        } else {
            ((written.saturating_mul(100)) / total).min(100) as u8
        };
        on_progress(SemanticSearchInstallProgress::Progress { percent });
    };
    download::download_from_url(
        &descriptor.url,
        &zip_path,
        descriptor.size,
        &descriptor.sha256,
        &mut progress,
    )
    .map_err(|error| SemanticSearchInstallError::Download(error.to_string()))?;
    check_canceled(canceled)?;
    stage_and_promote(install_root, descriptor, &zip_path)?;
    let _ = fs::remove_file(zip_path);
    on_progress(SemanticSearchInstallProgress::Progress { percent: 100 });
    on_progress(SemanticSearchInstallProgress::Installed);
    Ok(())
}

fn check_canceled(canceled: &AtomicBool) -> Result<(), SemanticSearchInstallError> {
    if canceled.load(Ordering::Relaxed) {
        return Err(SemanticSearchInstallError::Descriptor(
            "Semantic Search install canceled.".to_owned(),
        ));
    }
    Ok(())
}

fn validate_descriptor(
    descriptor: &SemanticSearchAssetDescriptor,
) -> Result<(), SemanticSearchInstallError> {
    if descriptor.url.trim().is_empty() {
        return Err(SemanticSearchInstallError::Descriptor(
            "Semantic Search download URL is not configured.".to_owned(),
        ));
    }
    if descriptor.size == 0 {
        return Err(SemanticSearchInstallError::Descriptor(
            "Semantic Search download size is not configured.".to_owned(),
        ));
    }
    if descriptor.sha256.len() != 64 || !descriptor.sha256.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Err(SemanticSearchInstallError::Descriptor(
            "Semantic Search download sha256 is not configured.".to_owned(),
        ));
    }
    if descriptor.model_key.trim().is_empty() {
        return Err(SemanticSearchInstallError::Descriptor(
            "Semantic Search model key is not configured.".to_owned(),
        ));
    }
    Ok(())
}

fn stage_and_promote(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    zip_path: &Path,
) -> Result<(), SemanticSearchInstallError> {
    let staging_root = install_paths::updates_dir(install_root).join(SEMANTIC_SEARCH_STAGING_NAME);
    if staging_root.exists() {
        fs::remove_dir_all(&staging_root)?;
    }
    fs::create_dir_all(&staging_root)?;
    match extract_zip(zip_path, &staging_root)
        .and_then(|()| validate_staged_model(&staging_root, descriptor))
        .and_then(|model_root| promote_staged_model(install_root, descriptor, &model_root))
    {
        Ok(()) => {
            let _ = fs::remove_dir_all(staging_root);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_dir_all(staging_root);
            Err(error)
        }
    }
}

fn extract_zip(zip_path: &Path, staging_root: &Path) -> Result<(), SemanticSearchInstallError> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| SemanticSearchInstallError::InvalidZip(error.to_string()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| SemanticSearchInstallError::InvalidZip(error.to_string()))?;
        let raw_name = entry.name().to_owned();
        let safe_relative = sanitize_entry_path(&raw_name)?;
        let out_path = staging_root.join(safe_relative);
        if entry.is_dir() {
            fs::create_dir_all(out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = File::create(out_path)?;
        io::copy(&mut entry, &mut out_file)?;
    }
    Ok(())
}

fn sanitize_entry_path(name: &str) -> Result<PathBuf, SemanticSearchInstallError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.starts_with('\\')
        || (name.len() >= 2 && name.as_bytes()[1] == b':')
    {
        return Err(SemanticSearchInstallError::PathTraversal {
            entry: name.to_owned(),
        });
    }
    let normalized = name.replace('\\', "/");
    let mut clean = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(SemanticSearchInstallError::PathTraversal {
                    entry: name.to_owned(),
                });
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(SemanticSearchInstallError::PathTraversal {
            entry: name.to_owned(),
        });
    }
    Ok(clean)
}

fn validate_staged_model(
    staging_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
) -> Result<PathBuf, SemanticSearchInstallError> {
    let model_root = staging_root.join("models").join(&descriptor.model_key);
    let required_files = DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files();
    for file in required_files {
        let path = model_root.join(file);
        if !path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
        {
            return Err(SemanticSearchInstallError::InvalidLayout(format!(
                "Semantic Search download is missing models/{}/{}",
                descriptor.model_key, file
            )));
        }
    }
    Ok(model_root)
}

fn promote_staged_model(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    staged_model_root: &Path,
) -> Result<(), SemanticSearchInstallError> {
    let store_root = model_store_root(install_root);
    fs::create_dir_all(&store_root)?;
    let final_model_root = store_root.join(&descriptor.model_key);
    let installing_model_root = store_root.join(format!("{}.installing", descriptor.model_key));
    if installing_model_root.exists() {
        fs::remove_dir_all(&installing_model_root)?;
    }
    fs::rename(staged_model_root, &installing_model_root).or_else(|_| {
        copy_dir_recursive(staged_model_root, &installing_model_root)?;
        fs::remove_dir_all(staged_model_root)
    })?;
    if final_model_root.exists() {
        fs::remove_dir_all(&final_model_root)?;
    }
    fs::rename(&installing_model_root, &final_model_root).or_else(|_| {
        copy_dir_recursive(&installing_model_root, &final_model_root)?;
        fs::remove_dir_all(&installing_model_root)
    })?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), io::Error> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(from, to)?;
        }
    }
    Ok(())
}

fn cleanup_install_paths(install_root: &Path, model_key: &str) {
    let updates_dir = install_paths::updates_dir(install_root);
    let _ = fs::remove_file(updates_dir.join(SEMANTIC_SEARCH_DOWNLOAD_NAME));
    let _ = fs::remove_dir_all(updates_dir.join(SEMANTIC_SEARCH_STAGING_NAME));
    let _ =
        fs::remove_dir_all(model_store_root(install_root).join(format!("{model_key}.installing")));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "snappyref-semantic-install-{prefix}-{}-{}-{nanos}",
            std::process::id(),
            counter
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, payload) in entries {
            if payload.is_empty() && name.ends_with('/') {
                writer.add_directory(*name, options()).unwrap();
            } else {
                writer.start_file(*name, options()).unwrap();
                writer.write_all(payload).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    fn descriptor_for(zip_path: &Path) -> SemanticSearchAssetDescriptor {
        let bytes = fs::read(zip_path).unwrap();
        SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: download::sha256_hex(&bytes),
            size: bytes.len() as u64,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        }
    }

    fn model_entries<'a>() -> Vec<(&'a str, &'a [u8])> {
        vec![
            ("models/", &[][..]),
            (
                "models/siglip2_base_patch16_224_int8/tokenizer.json",
                b"tokenizer",
            ),
            (
                "models/siglip2_base_patch16_224_int8/onnx/text_model_int8.onnx",
                b"text",
            ),
            (
                "models/siglip2_base_patch16_224_int8/onnx/vision_model_int8.onnx",
                b"vision",
            ),
        ]
    }

    #[test]
    fn staged_semantic_search_zip_promotes_valid_local_model_store_layout() {
        let root = unique_temp_dir("promote");
        let zip_path = root.join("semantic.zip");
        write_zip(&zip_path, &model_entries());
        let descriptor = descriptor_for(&zip_path);

        stage_and_promote(&root, &descriptor, &zip_path).unwrap();

        let model_root = root.join("models").join(&descriptor.model_key);
        assert!(model_root.join("tokenizer.json").is_file());
        assert!(model_root.join("onnx/text_model_int8.onnx").is_file());
        assert!(model_root.join("onnx/vision_model_int8.onnx").is_file());
        assert!(
            !install_paths::updates_dir(&root)
                .join(SEMANTIC_SEARCH_STAGING_NAME)
                .exists()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn staged_semantic_search_zip_rejects_unsafe_paths_and_removes_staging() {
        let root = unique_temp_dir("unsafe");
        let zip_path = root.join("semantic.zip");
        write_zip(
            &zip_path,
            &[
                ("models/", &[][..]),
                ("models/siglip2_base_patch16_224_int8/tokenizer.json", b"ok"),
                ("models/../escape.txt", b"no"),
            ],
        );
        let descriptor = descriptor_for(&zip_path);

        let error = stage_and_promote(&root, &descriptor, &zip_path).unwrap_err();

        assert!(error.to_string().contains("unsafe zip path"));
        assert!(!root.join("escape.txt").exists());
        assert!(
            !install_paths::updates_dir(&root)
                .join(SEMANTIC_SEARCH_STAGING_NAME)
                .exists()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removing_semantic_search_assets_keeps_library_and_embeddings() {
        let root = unique_temp_dir("remove");
        let descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        let model_root = root.join("models").join(&descriptor.model_key);
        fs::create_dir_all(model_root.join("onnx")).unwrap();
        fs::write(model_root.join("tokenizer.json"), b"tokenizer").unwrap();
        fs::write(model_root.join("onnx/text_model_int8.onnx"), b"text").unwrap();
        let library_root = root.join("library");
        fs::create_dir_all(library_root.join("embeddings")).unwrap();
        fs::write(library_root.join("embeddings/cache.bin"), b"embedding").unwrap();

        remove_semantic_search_assets(&root, &descriptor).unwrap();

        assert!(!model_root.exists());
        assert!(library_root.join("embeddings/cache.bin").is_file());

        let _ = fs::remove_dir_all(root);
    }
}
