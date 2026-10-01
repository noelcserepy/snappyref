//! Availability in the standalone Local Model Store. Checks never modify files.

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

pub(crate) fn refresh_local_model_store_status(
    data_root: &Path,
    model_key: &str,
    required_files: &[&str],
) -> LocalModelStoreStatus {
    let store = model_store_root(data_root);
    if required_files.iter().all(|relative| {
        store
            .join(model_key)
            .join(relative)
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
    }) {
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
        let status = || refresh_local_model_store_status(&root, "model", &["weights"]);

        assert_eq!(status(), LocalModelStoreStatus::Missing);
        assert!(!root.join("models").exists());

        let model = root.join("models/model");
        std::fs::create_dir_all(&model).unwrap();
        std::fs::write(model.join("weights"), "").unwrap();
        assert!(matches!(status(), LocalModelStoreStatus::Failed(_)));

        std::fs::write(model.join("weights"), "weights").unwrap();
        assert_eq!(status(), LocalModelStoreStatus::Installed);
    }
}
