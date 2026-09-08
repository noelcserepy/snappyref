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
    fn availability_preserves_bundled_files_and_requires_standalone_store() {
        let root = TempRoot::new("model-store-isolation");
        let bundled = root.join("SnappyRef.app/Contents/Resources/models/model");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::write(bundled.join("weights"), "bundled").unwrap();
        assert_eq!(
            refresh_local_model_store_status(&root, "model", &["weights"]),
            LocalModelStoreStatus::Missing
        );
        assert!(!root.join("models").exists());
        let standalone = root.join("models/model");
        std::fs::create_dir_all(&standalone).unwrap();
        std::fs::write(standalone.join("weights"), "standalone").unwrap();
        assert_eq!(
            refresh_local_model_store_status(&root, "model", &["weights"]),
            LocalModelStoreStatus::Installed
        );
        assert_eq!(
            std::fs::read_to_string(bundled.join("weights")).unwrap(),
            "bundled"
        );
        assert_eq!(
            std::fs::read_to_string(standalone.join("weights")).unwrap(),
            "standalone"
        );
    }

    #[test]
    fn incomplete_standalone_store_reports_failure_without_changing_files() {
        let root = TempRoot::new("incomplete-model-store");
        let store = root.join("models/model");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(store.join("weights"), "").unwrap();
        assert!(matches!(
            refresh_local_model_store_status(&root, "model", &["weights"]),
            LocalModelStoreStatus::Failed(_)
        ));
        assert_eq!(
            std::fs::read(store.join("weights")).unwrap(),
            Vec::<u8>::new()
        );
    }
}
