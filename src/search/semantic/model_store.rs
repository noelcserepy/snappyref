//! Availability in the Local Model Store. Startup recovers interrupted promotions; checks are read-only.

use std::path::{Path, PathBuf};

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
    if model_files_complete(&final_model, required_files) {
        return Ok(());
    }
    for suffix in ["previous", "installing"] {
        let recovery = store.join(format!("{model_key}.{suffix}"));
        if model_files_complete(&recovery, required_files) {
            match std::fs::symlink_metadata(&final_model) {
                Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(&final_model)?,
                Ok(_) => std::fs::remove_file(&final_model)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            // Both paths are in the model store, so recovery is an atomic rename.
            std::fs::rename(recovery, &final_model)?;
            break;
        }
    }
    Ok(())
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
