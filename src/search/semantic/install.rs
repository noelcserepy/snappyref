#![cfg_attr(test, allow(dead_code))]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::zip_extraction::{self, ExtractError};

use super::model_store::{model_store_root, recover_interrupted_model_promotion, remove_path_any};
use crate::search::semantic::DEFAULT_SEMANTIC_SEARCH_MODEL;
use crate::updater::{download, install_paths};

// Semantic Search reuses the updater download/staging helpers, but its
// `updates/` bucket lives under the writable data root with `models/`, not
// under the executable install root used by app auto-update.
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

impl From<ExtractError> for SemanticSearchInstallError {
    fn from(error: ExtractError) -> Self {
        match error {
            ExtractError::Io(error) => Self::Io(error),
            ExtractError::InvalidZip(error) => Self::InvalidZip(error),
            ExtractError::PathTraversal { entry } => Self::PathTraversal { entry },
            ExtractError::InvalidLayout(error) => Self::InvalidLayout(error),
        }
    }
}

impl SemanticSearchAssetDescriptor {
    /// The download size as the UI quotes it.
    pub(crate) fn megabytes(&self) -> u64 {
        self.size / 1_000_000
    }
}

pub(crate) fn default_semantic_search_asset_descriptor() -> SemanticSearchAssetDescriptor {
    SemanticSearchAssetDescriptor {
        url: env!("SNAPPYREF_SEMANTIC_SEARCH_URL").to_owned(),
        sha256: env!("SNAPPYREF_SEMANTIC_SEARCH_SHA256").to_owned(),
        size: env!("SNAPPYREF_SEMANTIC_SEARCH_SIZE")
            .parse()
            .expect("build validates model size"),
        model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
    }
}

pub(crate) fn remove_semantic_search_assets(
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
) -> Result<(), String> {
    cleanup_install_paths(data_root, &descriptor.model_key);
    for name in [
        descriptor.model_key.clone(),
        format!("{}.previous", descriptor.model_key),
        format!("{}.installing", descriptor.model_key),
        format!("{}.preparing", descriptor.model_key),
    ] {
        let model_root = model_store_root(data_root).join(name);
        remove_path_any(&model_root)
            .map_err(|error| format!("failed to remove {}: {error}", model_root.display()))?;
    }
    Ok(())
}

pub(crate) fn install_semantic_search_asset<F>(
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    canceled: &AtomicBool,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(SemanticSearchInstallProgress),
{
    install_inner(data_root, descriptor, canceled, &mut on_progress).map_err(|error| {
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
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    canceled: &AtomicBool,
    on_progress: &mut dyn FnMut(SemanticSearchInstallProgress),
) -> Result<(), SemanticSearchInstallError> {
    recover_interrupted_model_promotion(
        data_root,
        &descriptor.model_key,
        &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files(),
    )?;
    cleanup_install_paths(data_root, &descriptor.model_key);
    check_canceled(canceled)?;
    let updates_dir = install_paths::updates_dir(data_root);
    fs::create_dir_all(&updates_dir)?;
    let attempt_id = unique_install_attempt_id();
    let zip_path = updates_dir.join(format!("{SEMANTIC_SEARCH_DOWNLOAD_PREFIX}{attempt_id}.zip"));
    let staging_root = updates_dir.join(format!("{SEMANTIC_SEARCH_STAGING_PREFIX}{attempt_id}"));
    let mut progress = |written: u64, total: u64| {
        let percent = written
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0)
            .min(100) as u8;
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
    .and_then(|_| stage_and_promote(data_root, descriptor, &zip_path, &staging_root));

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

fn stage_and_promote(
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    zip_path: &Path,
    staging_root: &Path,
) -> Result<(), SemanticSearchInstallError> {
    if staging_root.exists() {
        fs::remove_dir_all(staging_root)?;
    }
    fs::create_dir_all(staging_root)?;
    match zip_extraction::extract(zip_path, staging_root, false)
        .map_err(SemanticSearchInstallError::from)
        .and_then(|()| validate_staged_model(staging_root, descriptor))
        .and_then(|model_root| promote_staged_model(data_root, descriptor, &model_root))
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
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    staged_model_root: &Path,
) -> Result<(), SemanticSearchInstallError> {
    promote_staged_model_with_hooks(
        data_root,
        descriptor,
        staged_model_root,
        |from, to| fs::rename(from, to),
        || {},
    )
}

fn promote_staged_model_with_hooks(
    data_root: &Path,
    descriptor: &SemanticSearchAssetDescriptor,
    staged_model_root: &Path,
    move_staged_model: impl FnOnce(&Path, &Path) -> io::Result<()>,
    before_final_replace: impl FnOnce(),
) -> Result<(), SemanticSearchInstallError> {
    recover_interrupted_model_promotion(
        data_root,
        &descriptor.model_key,
        &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files(),
    )?;
    let store_root = model_store_root(data_root);
    fs::create_dir_all(&store_root)?;
    let final_model_root = store_root.join(&descriptor.model_key);
    let installing_model_root = store_root.join(format!("{}.installing", descriptor.model_key));
    let preparing_model_root = store_root.join(format!("{}.preparing", descriptor.model_key));
    let backup_model_root = store_root.join(format!("{}.previous", descriptor.model_key));
    remove_path_any(&installing_model_root)?;
    remove_path_any(&preparing_model_root)?;
    move_staged_model(staged_model_root, &installing_model_root).or_else(|_| {
        copy_dir_recursive(staged_model_root, &preparing_model_root)?;
        fs::rename(&preparing_model_root, &installing_model_root)?;
        fs::remove_dir_all(staged_model_root)
    })?;

    let had_existing_model = final_model_root.exists();
    if final_model_root.exists() {
        remove_path_any(&backup_model_root)?;
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

fn cleanup_install_paths(data_root: &Path, model_key: &str) {
    let updates_dir = install_paths::updates_dir(data_root);
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
    for suffix in ["preparing", "installing"] {
        let _ = remove_path_any(&model_store_root(data_root).join(format!("{model_key}.{suffix}")));
    }
}

fn cleanup_attempt_paths(zip_path: &Path, staging_root: &Path) {
    let _ = fs::remove_file(zip_path);
    let _ = remove_path_any(staging_root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::channel;
    use std::thread;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

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

    fn descriptor_for_bytes(bytes: &[u8]) -> SemanticSearchAssetDescriptor {
        SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: download::sha256_hex(bytes),
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
    fn semantic_search_asset_download_success_installs_local_model_store() {
        let root = TempRoot::new("download-ok");
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
        assert_eq!(
            fs::read(model_root.join("tokenizer.json")).unwrap(),
            b"tokenizer"
        );
        assert_eq!(
            fs::read(model_root.join("onnx/text_model_int8.onnx")).unwrap(),
            b"text"
        );
        assert_eq!(
            fs::read(model_root.join("onnx/vision_model_int8.onnx")).unwrap(),
            b"vision"
        );
        assert!(progress.contains(&SemanticSearchInstallProgress::Installed));
        assert!(!install_artifacts_exist(&root));
    }

    #[test]
    fn semantic_search_asset_download_rejects_size_mismatch_and_cleans_up() {
        let root = TempRoot::new("download-size");
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
    }

    #[test]
    fn semantic_search_asset_download_rejects_corrupt_zip_and_removes_staging() {
        let root = TempRoot::new("download-corrupt");
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
    }

    #[test]
    fn canceled_semantic_search_install_cleans_partial_paths() {
        let root = TempRoot::new("cancel-cleanup");
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
    }

    #[test]
    fn semantic_search_install_reports_unwritable_data_root() {
        let root = TempRoot::new("unwritable-root");
        let data_root = root.join("SnappyRef");
        fs::write(&data_root, b"not a directory").unwrap();
        let descriptor = SemanticSearchAssetDescriptor {
            url: "https://example.invalid/snappyref-semantic-search.zip".to_owned(),
            sha256: "0".repeat(64),
            size: 1,
            model_key: DEFAULT_SEMANTIC_SEARCH_MODEL.model_key.to_owned(),
        };
        let canceled = AtomicBool::new(false);

        let error =
            install_semantic_search_asset(&data_root, &descriptor, &canceled, |_| {}).unwrap_err();

        assert!(error.contains("io:"));
        assert!(!data_root.join("updates").exists());
    }

    #[test]
    fn failed_semantic_search_promotion_preserves_existing_local_model_store() {
        let root = TempRoot::new("promote-preserve");
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
        let error = promote_staged_model_with_hooks(
            &root,
            &descriptor,
            &staged_model_root,
            |from, to| fs::rename(from, to),
            || {
                remove_path_any(&installing_model_root).unwrap();
            },
        )
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
    }

    #[test]
    fn interrupted_model_promotion_recovers_before_cleanup_and_failed_retry() {
        use super::super::model_store::{
            LocalModelStoreStatus, initialize_local_model_store_status,
        };
        for (recoverable, partial_final, startup) in [
            ("previous", false, true),
            ("installing", false, true),
            ("previous", true, true),
            ("previous", false, false),
            ("installing", false, false),
        ] {
            let root = TempRoot::new("interrupted-promotion");
            let descriptor = descriptor_for_bytes(b"unused");
            let model = root.join("models").join(&descriptor.model_key);
            let recovery = root
                .join("models")
                .join(format!("{}.{}", descriptor.model_key, recoverable));
            fs::create_dir_all(recovery.join("onnx")).unwrap();
            fs::write(recovery.join("tokenizer.json"), b"old-tokenizer").unwrap();
            fs::write(recovery.join("onnx/text_model_int8.onnx"), b"old-text").unwrap();
            fs::write(recovery.join("onnx/vision_model_int8.onnx"), b"old-vision").unwrap();
            if recoverable == "previous" {
                let installing = root
                    .join("models")
                    .join(format!("{}.installing", descriptor.model_key));
                copy_dir_recursive(&recovery, &installing).unwrap();
                fs::write(installing.join("tokenizer.json"), b"new-tokenizer").unwrap();
            }
            if partial_final {
                fs::create_dir_all(&model).unwrap();
                fs::write(model.join("tokenizer.json"), b"partial").unwrap();
            }
            if startup {
                assert_eq!(
                    super::super::model_store::refresh_local_model_store_status(
                        &root,
                        &descriptor.model_key,
                        &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files()
                    ),
                    LocalModelStoreStatus::Failed(format!(
                        "Semantic Search assets are incomplete in {}.",
                        root.join("models").display()
                    ))
                );
                assert!(recovery.exists());
                assert_eq!(
                    initialize_local_model_store_status(
                        &root,
                        &descriptor.model_key,
                        &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files()
                    ),
                    LocalModelStoreStatus::Installed
                );
            }
            let canceled = AtomicBool::new(true);
            let mut progress = Vec::new();
            assert_eq!(
                install_semantic_search_asset(&root, &descriptor, &canceled, |event| progress
                    .push(event))
                .unwrap_err(),
                "Semantic Search install canceled."
            );
            assert_eq!(progress, vec![SemanticSearchInstallProgress::Canceled]);
            assert_eq!(
                super::super::model_store::refresh_local_model_store_status(
                    &root,
                    &descriptor.model_key,
                    &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files()
                ),
                LocalModelStoreStatus::Installed
            );
            assert!(
                promote_staged_model(&root, &descriptor, &root.join("missing-staging")).is_err()
            );
            assert_eq!(
                fs::read(model.join("tokenizer.json")).unwrap(),
                b"old-tokenizer"
            );
            assert_eq!(
                fs::read(model.join("onnx/text_model_int8.onnx")).unwrap(),
                b"old-text"
            );
            assert_eq!(
                fs::read(model.join("onnx/vision_model_int8.onnx")).unwrap(),
                b"old-vision"
            );
        }
    }

    #[test]
    fn failed_retry_after_interrupted_copy_keeps_the_previous_model_bytes() {
        let root = TempRoot::new("interrupted-copy");
        let descriptor = descriptor_for_bytes(b"unused");
        let model = root.join("models").join(&descriptor.model_key);
        let previous = root
            .join("models")
            .join(format!("{}.previous", descriptor.model_key));
        let staged = root.join("staged");
        let installing = root
            .join("models")
            .join(format!("{}.installing", descriptor.model_key));
        let fixtures: [(&Path, &[u8], &[u8], &[u8]); 3] = [
            (&previous, b"old-tokenizer", b"old-text", b"old-vision"),
            (
                &model,
                b"partial-tokenizer",
                b"partial-text",
                b"partial-vision",
            ),
            (&staged, b"new-tokenizer", b"new-text", b"new-vision"),
        ];
        for (directory, tokenizer, text, vision) in fixtures {
            fs::create_dir_all(directory.join("onnx")).unwrap();
            fs::write(directory.join("tokenizer.json"), tokenizer).unwrap();
            fs::write(directory.join("onnx/text_model_int8.onnx"), text).unwrap();
            fs::write(directory.join("onnx/vision_model_int8.onnx"), vision).unwrap();
        }
        let error = promote_staged_model_with_hooks(
            &root,
            &descriptor,
            &staged,
            |from, to| fs::rename(from, to),
            || {
                remove_path_any(&installing).unwrap();
            },
        )
        .unwrap_err();
        assert!(matches!(error, SemanticSearchInstallError::Io(_)));
        assert_eq!(
            fs::read(model.join("tokenizer.json")).unwrap(),
            b"old-tokenizer"
        );
        assert_eq!(
            fs::read(model.join("onnx/text_model_int8.onnx")).unwrap(),
            b"old-text"
        );
        assert_eq!(
            fs::read(model.join("onnx/vision_model_int8.onnx")).unwrap(),
            b"old-vision"
        );
    }

    #[test]
    fn first_install_recovers_ready_model_over_nonempty_partial_final() {
        use super::super::model_store::{
            LocalModelStoreStatus, initialize_local_model_store_status,
        };
        let root = TempRoot::new("first-install-copy");
        let descriptor = descriptor_for_bytes(b"unused");
        let model = root.join("models").join(&descriptor.model_key);
        let installing = root
            .join("models")
            .join(format!("{}.installing", descriptor.model_key));
        for (directory, tokenizer, text, vision) in [
            (&model, &b"n"[..], &b"n"[..], &b"n"[..]),
            (
                &installing,
                &b"new-tokenizer"[..],
                &b"new-text"[..],
                &b"new-vision"[..],
            ),
        ] {
            fs::create_dir_all(directory.join("onnx")).unwrap();
            fs::write(directory.join("tokenizer.json"), tokenizer).unwrap();
            fs::write(directory.join("onnx/text_model_int8.onnx"), text).unwrap();
            fs::write(directory.join("onnx/vision_model_int8.onnx"), vision).unwrap();
        }
        assert_eq!(
            initialize_local_model_store_status(
                &root,
                &descriptor.model_key,
                &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files()
            ),
            LocalModelStoreStatus::Installed
        );
        assert_eq!(
            fs::read(model.join("tokenizer.json")).unwrap(),
            b"new-tokenizer"
        );
        assert_eq!(
            fs::read(model.join("onnx/text_model_int8.onnx")).unwrap(),
            b"new-text"
        );
        assert_eq!(
            fs::read(model.join("onnx/vision_model_int8.onnx")).unwrap(),
            b"new-vision"
        );
    }

    #[test]
    fn staging_copy_failure_keeps_old_model_and_successful_retry_publishes_ready_bytes() {
        let root = TempRoot::new("staging-copy-failure");
        let descriptor = descriptor_for_bytes(b"unused");
        let model = root.join("models").join(&descriptor.model_key);
        let installing = root
            .join("models")
            .join(format!("{}.installing", descriptor.model_key));
        let staged = root.join("staged");
        fs::create_dir_all(model.join("onnx")).unwrap();
        fs::write(model.join("tokenizer.json"), b"old-tokenizer").unwrap();
        fs::write(model.join("onnx/text_model_int8.onnx"), b"old-text").unwrap();
        fs::write(model.join("onnx/vision_model_int8.onnx"), b"old-vision").unwrap();

        assert!(matches!(
            promote_staged_model(&root, &descriptor, &staged),
            Err(SemanticSearchInstallError::Io(_))
        ));
        assert!(!installing.exists());
        assert_eq!(
            fs::read(model.join("tokenizer.json")).unwrap(),
            b"old-tokenizer"
        );
        assert_eq!(
            fs::read(model.join("onnx/text_model_int8.onnx")).unwrap(),
            b"old-text"
        );
        assert_eq!(
            fs::read(model.join("onnx/vision_model_int8.onnx")).unwrap(),
            b"old-vision"
        );

        fs::create_dir_all(staged.join("onnx")).unwrap();
        fs::write(staged.join("tokenizer.json"), b"new-tokenizer").unwrap();
        fs::write(staged.join("onnx/text_model_int8.onnx"), b"new-text").unwrap();
        fs::write(staged.join("onnx/vision_model_int8.onnx"), b"new-vision").unwrap();
        promote_staged_model_with_hooks(
            &root,
            &descriptor,
            &staged,
            |_, _| Err(io::Error::other("move unavailable")),
            || {
                assert_eq!(
                    fs::read(installing.join("tokenizer.json")).unwrap(),
                    b"new-tokenizer"
                );
                assert_eq!(
                    fs::read(installing.join("onnx/text_model_int8.onnx")).unwrap(),
                    b"new-text"
                );
                assert_eq!(
                    fs::read(installing.join("onnx/vision_model_int8.onnx")).unwrap(),
                    b"new-vision"
                );
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(model.join("tokenizer.json")).unwrap(),
            b"new-tokenizer"
        );
        assert_eq!(
            fs::read(model.join("onnx/text_model_int8.onnx")).unwrap(),
            b"new-text"
        );
        assert_eq!(
            fs::read(model.join("onnx/vision_model_int8.onnx")).unwrap(),
            b"new-vision"
        );
    }

    #[test]
    fn removing_semantic_search_assets_keeps_library_and_embeddings() {
        let root = TempRoot::new("remove");
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
        fs::write(model_root.join("onnx/vision_model_int8.onnx"), b"vision").unwrap();
        let previous = root
            .join("models")
            .join(format!("{}.previous", descriptor.model_key));
        copy_dir_recursive(&model_root, &previous).unwrap();
        let preparing = root
            .join("models")
            .join(format!("{}.preparing", descriptor.model_key));
        fs::create_dir_all(&preparing).unwrap();
        fs::write(preparing.join("tokenizer.json"), b"partial").unwrap();
        let library_root = root.join("library");
        let library = crate::library::storage::Library::initialize(library_root.clone()).unwrap();
        let path = library
            .store_rgba_image("red", &[255, 0, 0, 255], 1, 1)
            .unwrap();
        let asset = library
            .register_original(&path, "image/png", 1, 1, 4, None)
            .unwrap();
        library
            .upsert_asset_embedding_blob(asset.id, &descriptor.model_key, &[1.0, -0.5])
            .unwrap();
        drop(library);

        remove_semantic_search_assets(&root, &descriptor).unwrap();

        assert_eq!(
            super::super::model_store::initialize_local_model_store_status(
                &root,
                &descriptor.model_key,
                &DEFAULT_SEMANTIC_SEARCH_MODEL.required_model_files()
            ),
            super::super::model_store::LocalModelStoreStatus::Missing
        );
        let reopened = crate::library::storage::Library::initialize(library_root).unwrap();
        assert_eq!(
            reopened
                .load_asset_embedding(asset.id, &descriptor.model_key)
                .unwrap(),
            Some(vec![0, 0, 128, 63, 0, 0, 0, 191])
        );
        assert_eq!(reopened.load_asset(asset.id).unwrap(), asset);
    }
}
