#![cfg_attr(test, allow(dead_code))]

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zip::ZipArchive;

use super::model_store::model_store_root;
use crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL;
use crate::updater::{download, install_paths};

const DEFAULT_SEMANTIC_SEARCH_ASSET_URL: &str = "https://github.com/noelcserepy/snappyref-dist/releases/download/semantic-search/snappyref-semantic-search.zip";
const DEFAULT_SEMANTIC_SEARCH_ASSET_SHA256: &str =
    "252bbd840e67c09041feaf2e127f791903dab31d50bffb4810a162f1f1999429";
const DEFAULT_SEMANTIC_SEARCH_ASSET_SIZE: u64 = 253_732_373;

const SEMANTIC_SEARCH_DOWNLOAD_PREFIX: &str = "semantic-search-download-";
const SEMANTIC_SEARCH_STAGING_PREFIX: &str = "semantic-search-staging-";

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
            .unwrap_or(DEFAULT_SEMANTIC_SEARCH_ASSET_SHA256)
            .to_owned(),
        size: option_env!("SNAPPYREF_SEMANTIC_SEARCH_SIZE")
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(DEFAULT_SEMANTIC_SEARCH_ASSET_SIZE),
        model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
    }
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
    cleanup_install_paths(install_root, &descriptor.model_key);
    check_canceled(canceled)?;
    let updates_dir = install_paths::updates_dir(install_root);
    fs::create_dir_all(&updates_dir)?;
    let attempt_id = unique_install_attempt_id();
    let zip_path = updates_dir.join(format!("{SEMANTIC_SEARCH_DOWNLOAD_PREFIX}{attempt_id}.zip"));
    let staging_root = updates_dir.join(format!("{SEMANTIC_SEARCH_STAGING_PREFIX}{attempt_id}"));
    let mut progress = |written: u64, total: u64| {
        let percent = if total == 0 {
            0
        } else {
            ((written.saturating_mul(100)) / total).min(100) as u8
        };
        on_progress(SemanticSearchInstallProgress::Progress { percent });
    };
    let result = download::download_from_url_cancellable(
        &descriptor.url,
        &zip_path,
        descriptor.size,
        &descriptor.sha256,
        &mut progress,
        &|| canceled.load(Ordering::Relaxed),
    )
    .map_err(|error| SemanticSearchInstallError::Download(error.to_string()))
    .and_then(|_| check_canceled(canceled))
    .and_then(|_| stage_and_promote(install_root, descriptor, &zip_path, &staging_root));

    match result {
        Ok(()) => {
            let _ = fs::remove_file(zip_path);
            on_progress(SemanticSearchInstallProgress::Progress { percent: 100 });
            on_progress(SemanticSearchInstallProgress::Installed);
            Ok(())
        }
        Err(error) => {
            cleanup_attempt_paths(&zip_path, &staging_root);
            Err(error)
        }
    }
}

fn unique_install_attempt_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
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
    staging_root: &Path,
) -> Result<(), SemanticSearchInstallError> {
    if staging_root.exists() {
        fs::remove_dir_all(staging_root)?;
    }
    fs::create_dir_all(staging_root)?;
    match extract_zip(zip_path, staging_root)
        .and_then(|()| validate_staged_model(staging_root, descriptor))
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
    promote_staged_model_with_hook(install_root, descriptor, staged_model_root, || {})
}

fn promote_staged_model_with_hook(
    install_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    staged_model_root: &Path,
    before_final_replace: impl FnOnce(),
) -> Result<(), SemanticSearchInstallError> {
    let store_root = model_store_root(install_root);
    fs::create_dir_all(&store_root)?;
    let final_model_root = store_root.join(&descriptor.model_key);
    let installing_model_root = store_root.join(format!("{}.installing", descriptor.model_key));
    let backup_model_root = store_root.join(format!("{}.previous", descriptor.model_key));
    if installing_model_root.exists() {
        remove_path_any(&installing_model_root)?;
    }
    if backup_model_root.exists() {
        remove_path_any(&backup_model_root)?;
    }
    fs::rename(staged_model_root, &installing_model_root).or_else(|_| {
        copy_dir_recursive(staged_model_root, &installing_model_root)?;
        fs::remove_dir_all(staged_model_root)
    })?;

    let had_existing_model = final_model_root.exists();
    if final_model_root.exists() {
        fs::rename(&final_model_root, &backup_model_root)?;
    }

    before_final_replace();

    let promote_result = fs::rename(&installing_model_root, &final_model_root).or_else(|_| {
        copy_dir_recursive(&installing_model_root, &final_model_root)?;
        fs::remove_dir_all(&installing_model_root)
    });

    if let Err(error) = promote_result {
        let _ = remove_path_any(&final_model_root);
        if had_existing_model && backup_model_root.exists() {
            let _ = fs::rename(&backup_model_root, &final_model_root).or_else(|_| {
                copy_dir_recursive(&backup_model_root, &final_model_root)?;
                remove_path_any(&backup_model_root)
            });
        }
        return Err(SemanticSearchInstallError::Io(error));
    }

    if backup_model_root.exists() {
        remove_path_any(&backup_model_root)?;
    }
    Ok(())
}

fn remove_path_any(path: &Path) -> Result<(), io::Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
    if let Ok(entries) = fs::read_dir(&updates_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(SEMANTIC_SEARCH_DOWNLOAD_PREFIX)
                || name.starts_with(SEMANTIC_SEARCH_STAGING_PREFIX)
            {
                let _ = remove_path_any(&entry.path());
            }
        }
    }
    let _ =
        fs::remove_dir_all(model_store_root(install_root).join(format!("{model_key}.installing")));
}

fn cleanup_attempt_paths(zip_path: &Path, staging_root: &Path) {
    let _ = fs::remove_file(zip_path);
    let _ = remove_path_any(staging_root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::channel;
    use std::thread;
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
        descriptor_for_bytes(&bytes)
    }

    fn descriptor_for_bytes(bytes: &[u8]) -> SemanticSearchAssetDescriptor {
        SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: download::sha256_hex(&bytes),
            size: bytes.len() as u64,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        }
    }

    fn install_artifacts_exist(root: &Path) -> bool {
        let updates_dir = install_paths::updates_dir(root);
        fs::read_dir(updates_dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.flatten())
            .any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with(SEMANTIC_SEARCH_DOWNLOAD_PREFIX)
                    || name.starts_with(SEMANTIC_SEARCH_STAGING_PREFIX)
            })
    }

    #[test]
    fn default_semantic_search_descriptor_is_installable_contract() {
        let descriptor = default_semantic_search_asset_descriptor();

        assert_eq!(descriptor.url, DEFAULT_SEMANTIC_SEARCH_ASSET_URL);
        assert_eq!(descriptor.sha256, DEFAULT_SEMANTIC_SEARCH_ASSET_SHA256);
        assert_eq!(descriptor.size, DEFAULT_SEMANTIC_SEARCH_ASSET_SIZE);
        validate_descriptor(&descriptor).unwrap();
    }

    fn serve_once(payload: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/semantic.zip");
        let (ready_tx, ready_rx) = channel::<()>();
        let handle = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            if let Ok((stream, _)) = listener.accept() {
                handle_request(stream, &payload);
            }
        });
        ready_rx.recv().unwrap();
        (url, handle)
    }

    fn handle_request(mut stream: TcpStream, payload: &[u8]) {
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let _ = stream.write_all(headers.as_bytes());
        let _ = stream.write_all(payload);
        let _ = stream.flush();
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
    fn semantic_search_descriptor_rejects_missing_download_contract() {
        let mut descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };

        descriptor.url.clear();
        assert!(
            validate_descriptor(&descriptor)
                .unwrap_err()
                .to_string()
                .contains("URL is not configured")
        );

        descriptor.url = "https://example.invalid/snappyref-semantic-search.zip".to_owned();
        descriptor.size = 0;
        assert!(
            validate_descriptor(&descriptor)
                .unwrap_err()
                .to_string()
                .contains("size is not configured")
        );

        descriptor.size = 1;
        descriptor.sha256 = "not-a-sha".to_owned();
        assert!(
            validate_descriptor(&descriptor)
                .unwrap_err()
                .to_string()
                .contains("sha256 is not configured")
        );

        descriptor.sha256 = "0".repeat(64);
        descriptor.model_key.clear();
        assert!(
            validate_descriptor(&descriptor)
                .unwrap_err()
                .to_string()
                .contains("model key is not configured")
        );
    }

    #[test]
    fn semantic_search_asset_download_success_installs_local_model_store() {
        let root = unique_temp_dir("download-ok");
        let zip_path = root.join("semantic.zip");
        write_zip(&zip_path, &model_entries());
        let zip_bytes = fs::read(&zip_path).unwrap();
        let (url, handle) = serve_once(zip_bytes.clone());
        let mut descriptor = descriptor_for_bytes(&zip_bytes);
        descriptor.url = url;
        let canceled = AtomicBool::new(false);
        let mut progress = Vec::new();

        install_semantic_search_asset(&root, &descriptor, &canceled, |event| progress.push(event))
            .unwrap();
        let _ = handle.join();

        let model_root = root.join("models").join(&descriptor.model_key);
        assert!(model_root.join("tokenizer.json").is_file());
        assert!(model_root.join("onnx/text_model_int8.onnx").is_file());
        assert!(model_root.join("onnx/vision_model_int8.onnx").is_file());
        assert!(progress.contains(&SemanticSearchInstallProgress::Installed));
        assert!(!install_artifacts_exist(&root));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_search_asset_download_rejects_size_mismatch_and_cleans_up() {
        let root = unique_temp_dir("download-size");
        let zip_path = root.join("semantic.zip");
        write_zip(&zip_path, &model_entries());
        let zip_bytes = fs::read(&zip_path).unwrap();
        let (url, handle) = serve_once(zip_bytes.clone());
        let mut descriptor = descriptor_for_bytes(&zip_bytes);
        descriptor.url = url;
        descriptor.size += 1;
        let canceled = AtomicBool::new(false);
        let mut progress = Vec::new();

        let error = install_semantic_search_asset(&root, &descriptor, &canceled, |event| {
            progress.push(event)
        })
        .unwrap_err();
        let _ = handle.join();

        assert!(error.contains("size mismatch"));
        assert!(progress.iter().any(|event| matches!(
            event,
            SemanticSearchInstallProgress::Failed { error } if error.contains("size mismatch")
        )));
        assert!(!root.join("models").join(&descriptor.model_key).exists());
        assert!(!install_artifacts_exist(&root));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_search_asset_download_rejects_sha_mismatch_and_cleans_up() {
        let root = unique_temp_dir("download-sha");
        let zip_path = root.join("semantic.zip");
        write_zip(&zip_path, &model_entries());
        let zip_bytes = fs::read(&zip_path).unwrap();
        let (url, handle) = serve_once(zip_bytes);
        let mut descriptor = SemanticSearchAssetDescriptor {
            url,
            sha256: "f".repeat(64),
            size: fs::metadata(&zip_path).unwrap().len(),
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        let canceled = AtomicBool::new(false);

        let error =
            install_semantic_search_asset(&root, &descriptor, &canceled, |_| {}).unwrap_err();
        let _ = handle.join();

        assert!(error.contains("sha256 mismatch"));
        assert!(!root.join("models").join(&descriptor.model_key).exists());
        assert!(!install_artifacts_exist(&root));

        descriptor.url = "https://example.invalid/unused.zip".to_owned();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_search_asset_download_rejects_corrupt_zip_and_removes_staging() {
        let root = unique_temp_dir("download-corrupt");
        let zip_bytes = b"not a zip".to_vec();
        let (url, handle) = serve_once(zip_bytes.clone());
        let mut descriptor = descriptor_for_bytes(&zip_bytes);
        descriptor.url = url;
        let canceled = AtomicBool::new(false);

        let error =
            install_semantic_search_asset(&root, &descriptor, &canceled, |_| {}).unwrap_err();
        let _ = handle.join();

        assert!(error.contains("corrupt zip"));
        assert!(!root.join("models").join(&descriptor.model_key).exists());
        assert!(!install_artifacts_exist(&root));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canceled_semantic_search_install_cleans_partial_paths() {
        let root = unique_temp_dir("cancel-cleanup");
        let descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        fs::create_dir_all(
            install_paths::updates_dir(&root).join(format!("{SEMANTIC_SEARCH_STAGING_PREFIX}old")),
        )
        .unwrap();
        fs::write(
            install_paths::updates_dir(&root)
                .join(format!("{SEMANTIC_SEARCH_DOWNLOAD_PREFIX}old.zip")),
            b"partial",
        )
        .unwrap();
        fs::create_dir_all(
            root.join("models")
                .join(format!("{}.installing", descriptor.model_key)),
        )
        .unwrap();
        let canceled = AtomicBool::new(true);

        let error =
            install_semantic_search_asset(&root, &descriptor, &canceled, |_| {}).unwrap_err();

        assert!(error.contains("canceled"));
        assert!(!install_artifacts_exist(&root));
        assert!(
            !root
                .join("models")
                .join(format!("{}.installing", descriptor.model_key))
                .exists()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn semantic_search_install_reports_unwritable_install_root() {
        let root = unique_temp_dir("unwritable-root");
        let install_root = root.join("SnappyRef");
        fs::write(&install_root, b"not a directory").unwrap();
        let descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        let canceled = AtomicBool::new(false);

        let error = install_semantic_search_asset(&install_root, &descriptor, &canceled, |_| {})
            .unwrap_err();

        assert!(error.contains("io:"));
        assert!(!install_root.join("updates").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn staged_semantic_search_zip_promotes_valid_local_model_store_layout() {
        let root = unique_temp_dir("promote");
        let zip_path = root.join("semantic.zip");
        write_zip(&zip_path, &model_entries());
        let descriptor = descriptor_for(&zip_path);

        let staging_root = install_paths::updates_dir(&root).join("semantic-search-staging-test");
        stage_and_promote(&root, &descriptor, &zip_path, &staging_root).unwrap();

        let model_root = root.join("models").join(&descriptor.model_key);
        assert!(model_root.join("tokenizer.json").is_file());
        assert!(model_root.join("onnx/text_model_int8.onnx").is_file());
        assert!(model_root.join("onnx/vision_model_int8.onnx").is_file());
        assert!(!staging_root.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_semantic_search_promotion_preserves_existing_local_model_store() {
        let root = unique_temp_dir("promote-preserve");
        let descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        let final_model_root = root.join("models").join(&descriptor.model_key);
        fs::create_dir_all(final_model_root.join("onnx")).unwrap();
        fs::write(final_model_root.join("tokenizer.json"), b"old-tokenizer").unwrap();
        fs::write(
            final_model_root.join("onnx/text_model_int8.onnx"),
            b"old-text",
        )
        .unwrap();
        fs::write(
            final_model_root.join("onnx/vision_model_int8.onnx"),
            b"old-vision",
        )
        .unwrap();

        let staged_model_root = root
            .join("updates/staged/models")
            .join(&descriptor.model_key);
        fs::create_dir_all(staged_model_root.join("onnx")).unwrap();
        fs::write(staged_model_root.join("tokenizer.json"), b"new-tokenizer").unwrap();
        fs::write(
            staged_model_root.join("onnx/text_model_int8.onnx"),
            b"new-text",
        )
        .unwrap();
        fs::write(
            staged_model_root.join("onnx/vision_model_int8.onnx"),
            b"new-vision",
        )
        .unwrap();

        let installing_model_root = root
            .join("models")
            .join(format!("{}.installing", descriptor.model_key));
        let error = promote_staged_model_with_hook(&root, &descriptor, &staged_model_root, || {
            remove_path_any(&installing_model_root).unwrap();
        })
        .unwrap_err();

        assert!(matches!(error, SemanticSearchInstallError::Io(_)));
        assert_eq!(
            fs::read_to_string(final_model_root.join("tokenizer.json")).unwrap(),
            "old-tokenizer"
        );
        assert_eq!(
            fs::read_to_string(final_model_root.join("onnx/text_model_int8.onnx")).unwrap(),
            "old-text"
        );
        assert!(
            !root
                .join("models")
                .join(format!("{}.previous", descriptor.model_key))
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

        let staging_root = install_paths::updates_dir(&root).join("semantic-search-staging-test");
        let error = stage_and_promote(&root, &descriptor, &zip_path, &staging_root).unwrap_err();

        assert!(error.to_string().contains("unsafe zip path"));
        assert!(!root.join("escape.txt").exists());
        assert!(!staging_root.exists());

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
