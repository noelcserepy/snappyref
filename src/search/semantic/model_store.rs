//! Availability in the Local Model Store. Startup recovers interrupted promotions; checks are read-only.
//! Recovery prefers `.previous`, then marked ready `.installing`, then final.
//! New incoming copies use `.preparing` and become `.installing` atomically only
//! after copying completes; the readiness marker is written before final replacement.
//! Legacy unmarked `.installing` is only a fallback when final is incomplete;
//! required-file presence cannot prove that every legacy byte is complete.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const PROMOTION_READY_FILE: &str = ".promotion-ready";

pub(crate) fn model_store_root(data_root: &Path) -> PathBuf {
    data_root.join("models")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalModelStoreStatus {
    Installed,
    Missing,
    Downloading { percent: u8 },
    Failed(String),
}

impl LocalModelStoreStatus {
    pub(crate) fn assets_available(&self) -> bool {
        matches!(self, Self::Installed)
    }
}

pub(crate) fn initialize_local_model_store_status(
    data_root: &Path,
    model_key: &str,
    required_files: &[&str],
) -> LocalModelStoreStatus {
    if let Err(error) = recover_interrupted_model_promotion(data_root, model_key, required_files) {
        return LocalModelStoreStatus::Failed(format!(
            "failed to recover Semantic Search assets: {error}"
        ));
    }
    refresh_local_model_store_status(data_root, model_key, required_files)
}

fn model_files_complete(model_root: &Path, required_files: &[&str]) -> bool {
    required_files.iter().all(|relative| {
        model_root
            .join(relative)
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    })
}

pub(crate) fn recover_interrupted_model_promotion(
    data_root: &Path,
    model_key: &str,
    required_files: &[&str],
) -> std::io::Result<()> {
    let store = model_store_root(data_root);
    let final_model = store.join(model_key);
    let installing = store.join(format!("{model_key}.installing"));
    let marked_installing = installing.join(PROMOTION_READY_FILE).is_file();
    for (name, eligible) in [
        (format!("{model_key}.previous"), true),
        (format!("{model_key}.installing"), marked_installing),
        (model_key.to_owned(), true),
        (format!("{model_key}.installing"), !marked_installing),
    ] {
        if !eligible {
            continue;
        }
        let recovery = store.join(name);
        if model_files_complete(&recovery, required_files) {
            if recovery == final_model {
                return Ok(());
            }
            if recovery == store.join(format!("{model_key}.previous")) {
                return rollback_model_promotion(data_root, model_key);
            }
            remove_path_any(&final_model)?;
            // Both paths are in the model store, so recovery is an atomic rename.
            std::fs::rename(recovery, &final_model)?;
            break;
        }
    }
    Ok(())
}

pub(crate) fn rollback_model_promotion(data_root: &Path, model_key: &str) -> io::Result<()> {
    let store = model_store_root(data_root);
    // Discard the replacement before consuming its backup, including on cleanup failure.
    for suffix in ["installing", "preparing"] {
        remove_path_any(&store.join(format!("{model_key}.{suffix}")))?;
    }
    let final_model = store.join(model_key);
    remove_path_any(&final_model)?;
    // Both paths are in the model store; rollback does not need a copy fallback.
    fs::rename(store.join(format!("{model_key}.previous")), final_model)
}

pub(crate) fn remove_path_any(path: &Path) -> Result<(), io::Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn refresh_local_model_store_status(
    data_root: &Path,
    model_key: &str,
    required_files: &[&str],
) -> LocalModelStoreStatus {
    let store = model_store_root(data_root);
    if model_files_complete(&store.join(model_key), required_files) {
        return LocalModelStoreStatus::Installed;
    }
    if std::fs::read_dir(&store).is_ok_and(|mut entries| entries.next().is_some()) {
        return LocalModelStoreStatus::Failed(format!(
            "Semantic Search assets are incomplete in {}.",
            store.display()
        ));
    }
    LocalModelStoreStatus::Missing
}

pub(crate) const MISSING_MODEL_USER_MESSAGE: &str =
    "Semantic Search assets are not installed. Install them from Settings or the search palette.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    #[test]
    fn local_model_store_status_requires_every_file_with_content() {
        let root = TempRoot::new("model-store-status");
        let status = || refresh_local_model_store_status(&root, "model", &["weights", "tokenizer"]);

        assert_eq!(status(), LocalModelStoreStatus::Missing);
        assert!(!root.join("models").exists());

        let model = root.join("models/model");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("tokenizer"), "tokenizer").unwrap();
        std::fs::write(model.join("weights"), "").unwrap();
        assert_eq!(
            status(),
            LocalModelStoreStatus::Failed(format!(
                "Semantic Search assets are incomplete in {}.",
                root.join("models").display()
            ))
        );

        std::fs::remove_file(model.join("tokenizer")).unwrap();
        std::fs::write(model.join("weights"), "weights").unwrap();
        assert_eq!(
            status(),
            LocalModelStoreStatus::Failed(format!(
                "Semantic Search assets are incomplete in {}.",
                root.join("models").display()
            ))
        );
        std::fs::write(model.join("tokenizer"), "tokenizer").unwrap();
        assert_eq!(status(), LocalModelStoreStatus::Installed);
    }
}
