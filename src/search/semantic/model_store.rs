//! Local Model Store: resolves and migrates the on-disk model directory that
//! lives outside the app bundle at `<data_root>/models/`.
//!
//! This module is the single source of truth for:
//! - resolving `<data_root>/models/` across macOS `.app` and plain-binary layouts
//! - detecting an in-bundle model directory left over from older installs
//! - performing a one-shot, idempotent migration that moves the in-bundle model
//!   into the Local Model Store
//! - reporting a clear, user-facing error when no model is available
//!
//! The interface is unit-testable: every function that touches the filesystem
//! takes explicit paths so tests can simulate macOS and Windows layouts without
//! relying on `std::env::current_exe`.

use std::path::{Path, PathBuf};

/// The on-disk root of the Local Model Store relative to a given data root.
pub(crate) fn model_store_root(data_root: &Path) -> PathBuf {
    data_root.join("models")
}

/// The location of an in-bundle model directory if the running executable
/// looks like a macOS `.app` bundle. Returns `None` for plain-binary installs
/// (Windows / dev builds), which never had an in-bundle model directory.
pub(crate) fn bundled_model_dir(executable_dir: &Path) -> Option<PathBuf> {
    let contents_dir = executable_dir.parent()?;
    if executable_dir.file_name()? != "MacOS" || contents_dir.file_name()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    if app_dir.extension()? != "app" {
        return None;
    }
    Some(contents_dir.join("Resources").join("models"))
}

/// Outcome of [`migrate_bundled_model`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MigrationOutcome {
    /// No in-bundle model directory existed; nothing to migrate.
    NoBundledModel,
    /// The Local Model Store already exists; migration is a no-op (idempotent).
    AlreadyMigrated,
    /// Files were moved from the in-bundle directory into the Local Model Store.
    Migrated,
}

/// Status of model availability for a given data root + executable layout.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelAvailability {
    /// A Local Model Store exists at `<data_root>/models/`.
    LocalStore,
    /// No Local Model Store exists, but an in-bundle model dir is present and
    /// can be migrated.
    BundledOnly,
    /// Neither a Local Model Store nor a bundled model is available.
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalModelStoreStatus {
    Installed,
    InstalledViaMigration,
    Missing,
    #[allow(dead_code)]
    Downloading {
        percent: u8,
    },
    Failed(String),
}

impl LocalModelStoreStatus {
    pub(crate) fn assets_available(&self) -> bool {
        matches!(self, Self::Installed | Self::InstalledViaMigration)
    }
}

/// Inspect the filesystem and classify model availability.
pub(crate) fn classify_availability(data_root: &Path, executable_dir: &Path) -> ModelAvailability {
    let store = model_store_root(data_root);
    if dir_has_entries(&store) {
        return ModelAvailability::LocalStore;
    }
    if let Some(bundled) = bundled_model_dir(executable_dir) {
        if dir_has_entries(&bundled) {
            return ModelAvailability::BundledOnly;
        }
    }
    ModelAvailability::Missing
}

pub(crate) fn refresh_local_model_store_status(
    data_root: &Path,
    executable_dir: &Path,
    model_key: &str,
    required_files: &[&str],
) -> LocalModelStoreStatus {
    let migration = match migrate_bundled_model(data_root, executable_dir) {
        Ok(outcome) => outcome,
        Err(error) => {
            if !required_model_files_present(data_root, model_key, required_files) {
                return LocalModelStoreStatus::Failed(error);
            }
            MigrationOutcome::AlreadyMigrated
        }
    };

    if required_model_files_present(data_root, model_key, required_files) {
        return match migration {
            MigrationOutcome::Migrated => LocalModelStoreStatus::InstalledViaMigration,
            MigrationOutcome::NoBundledModel | MigrationOutcome::AlreadyMigrated => {
                LocalModelStoreStatus::Installed
            }
        };
    }

    if matches!(
        classify_availability(data_root, executable_dir),
        ModelAvailability::LocalStore | ModelAvailability::BundledOnly
    ) {
        return LocalModelStoreStatus::Failed(format!(
            "Semantic Search assets are incomplete in {}.",
            model_store_root(data_root).display()
        ));
    }

    LocalModelStoreStatus::Missing
}

/// Move the in-bundle `models/` directory (if any) to `<data_root>/models/`.
///
/// This is idempotent: if `<data_root>/models/` already has content it is
/// left alone and the in-bundle directory is removed. If no in-bundle model
/// exists, the function returns `NoBundledModel`.
pub(crate) fn migrate_bundled_model(
    data_root: &Path,
    executable_dir: &Path,
) -> Result<MigrationOutcome, String> {
    let Some(bundled) = bundled_model_dir(executable_dir) else {
        return Ok(MigrationOutcome::NoBundledModel);
    };
    if !dir_has_entries(&bundled) {
        return Ok(MigrationOutcome::NoBundledModel);
    }

    let store = model_store_root(data_root);
    if dir_has_entries(&store) {
        // Local Model Store already exists; remove the stale in-bundle copy and
        // report idempotent success.
        let _ = std::fs::remove_dir_all(&bundled);
        return Ok(MigrationOutcome::AlreadyMigrated);
    }

    if let Some(parent) = store.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to prepare Local Model Store parent {}: {error}",
                parent.display()
            )
        })?;
    }

    move_directory(&bundled, &store)?;
    Ok(MigrationOutcome::Migrated)
}

/// The user-facing error string surfaced when no model assets are available
/// anywhere outside the interactive install surfaces.
pub(crate) const MISSING_MODEL_USER_MESSAGE: &str =
    "Semantic Search assets are not installed. Install them from Settings or the search palette.";

fn dir_has_entries(path: &Path) -> bool {
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.next().is_some()
}

fn required_model_files_present(
    data_root: &Path,
    model_key: &str,
    required_files: &[&str],
) -> bool {
    let model_root = model_store_root(data_root).join(model_key);
    required_files.iter().all(|relative| {
        model_root
            .join(relative)
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false)
    })
}

fn move_directory(source: &Path, destination: &Path) -> Result<(), String> {
    // Prefer an atomic rename; fall back to a recursive copy + remove for the
    // cross-device case.
    if std::fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    copy_dir_recursive(source, destination)?;
    std::fs::remove_dir_all(source).map_err(|error| {
        format!(
            "failed to remove migrated source directory {}: {error}",
            source.display()
        )
    })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| {
        format!(
            "failed to create directory {}: {error}",
            destination.display()
        )
    })?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to iterate {}: {error}", source.display()))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("failed to stat {}: {error}", entry.path().display()))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|error| {
                format!(
                    "failed to copy {} -> {}: {error}",
                    from.display(),
                    to.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_root(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "snappyref-{prefix}-{}-{}-{nanos}",
            std::process::id(),
            counter
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    const TEST_MODEL_KEY: &str = "siglip2_base_patch16_224_int8";
    const TEST_REQUIRED_FILES: &[&str] = &[
        "tokenizer.json",
        "onnx/text_model_int8.onnx",
        "onnx/vision_model_int8.onnx",
    ];

    fn write_required_model_files(root: &Path) {
        for file in TEST_REQUIRED_FILES {
            write_file(&root.join(TEST_MODEL_KEY).join(file), "model");
        }
    }

    #[test]
    fn model_store_root_is_sibling_of_library_for_plain_binary() {
        let data_root = PathBuf::from("/fake/SnappyRef");
        assert_eq!(
            model_store_root(&data_root),
            PathBuf::from("/fake/SnappyRef/models"),
        );
    }

    #[test]
    fn model_store_root_for_macos_data_root_is_outside_app_bundle() {
        // On macOS the data root is the parent of SnappyRef.app — same shape
        // as the plain-binary case, so `<data_root>/models/` is a sibling of
        // SnappyRef.app, not nested inside it.
        let data_root = PathBuf::from("/Users/alice/SnappyRef");
        assert_eq!(
            model_store_root(&data_root),
            PathBuf::from("/Users/alice/SnappyRef/models"),
        );
    }

    #[test]
    fn bundled_model_dir_resolves_resources_path_for_macos_app() {
        let executable_dir = Path::new("/Users/alice/SnappyRef/SnappyRef.app/Contents/MacOS");
        assert_eq!(
            bundled_model_dir(executable_dir),
            Some(PathBuf::from(
                "/Users/alice/SnappyRef/SnappyRef.app/Contents/Resources/models",
            )),
        );
    }

    #[test]
    fn bundled_model_dir_is_none_for_plain_binary() {
        let executable_dir = Path::new("/fake/SnappyRef");
        assert!(bundled_model_dir(executable_dir).is_none());
    }

    #[test]
    fn classify_availability_reports_local_store_when_present() {
        let root = unique_temp_root("availability-local");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.clone();
        write_file(&data_root.join("models").join("marker.bin"), "data");

        assert_eq!(
            classify_availability(&data_root, &executable_dir),
            ModelAvailability::LocalStore,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_availability_reports_bundled_only_for_macos_app_layout() {
        let root = unique_temp_root("availability-bundled");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = data_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/marker.bin"), "data");

        assert_eq!(
            classify_availability(&data_root, &executable_dir),
            ModelAvailability::BundledOnly,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_availability_reports_missing_when_nothing_present() {
        let root = unique_temp_root("availability-missing");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();

        assert_eq!(
            classify_availability(&data_root, &executable_dir),
            ModelAvailability::Missing,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_model_store_status_reports_missing_without_creating_store() {
        let root = unique_temp_root("status-missing");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.clone();
        std::fs::create_dir_all(&data_root).unwrap();

        let status = refresh_local_model_store_status(
            &data_root,
            &executable_dir,
            TEST_MODEL_KEY,
            TEST_REQUIRED_FILES,
        );

        assert_eq!(status, LocalModelStoreStatus::Missing);
        assert!(!data_root.join("models").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_model_store_status_reports_installed_for_complete_store() {
        let root = unique_temp_root("status-installed");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.clone();
        write_required_model_files(&data_root.join("models"));

        let status = refresh_local_model_store_status(
            &data_root,
            &executable_dir,
            TEST_MODEL_KEY,
            TEST_REQUIRED_FILES,
        );

        assert_eq!(status, LocalModelStoreStatus::Installed);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_model_store_status_reports_installed_via_migration() {
        let root = unique_temp_root("status-migrated");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = data_root.join("SnappyRef.app/Contents/Resources/models");
        write_required_model_files(&bundled);

        let status = refresh_local_model_store_status(
            &data_root,
            &executable_dir,
            TEST_MODEL_KEY,
            TEST_REQUIRED_FILES,
        );

        assert_eq!(status, LocalModelStoreStatus::InstalledViaMigration);
        assert!(data_root.join("models").join(TEST_MODEL_KEY).exists());
        assert!(!bundled.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn local_model_store_status_reports_failed_for_incomplete_store() {
        let root = unique_temp_root("status-incomplete");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.clone();
        write_file(
            &data_root
                .join("models")
                .join(TEST_MODEL_KEY)
                .join("tokenizer.json"),
            "model",
        );

        let status = refresh_local_model_store_status(
            &data_root,
            &executable_dir,
            TEST_MODEL_KEY,
            TEST_REQUIRED_FILES,
        );

        assert!(matches!(status, LocalModelStoreStatus::Failed(_)));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_moves_files_to_local_store_once() {
        let root = unique_temp_root("migrate-once");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = data_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "{}");
        write_file(&bundled.join("siglip2/onnx/text_model.onnx"), "weights");

        let outcome = migrate_bundled_model(&data_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::Migrated);
        let store = data_root.join("models");
        assert!(store.join("siglip2/tokenizer.json").exists());
        assert!(store.join("siglip2/onnx/text_model.onnx").exists());
        // Source must be gone after a successful migration.
        assert!(!bundled.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_is_idempotent_when_store_already_present() {
        let root = unique_temp_root("migrate-idempotent");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();

        // Pre-existing Local Model Store with a known marker.
        write_file(
            &data_root.join("models/siglip2/tokenizer.json"),
            "preserved",
        );
        // Stale in-bundle copy with different contents.
        let bundled = data_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "stale");

        let outcome = migrate_bundled_model(&data_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::AlreadyMigrated);
        // Local Model Store contents are preserved (not overwritten).
        assert_eq!(
            std::fs::read_to_string(data_root.join("models/siglip2/tokenizer.json")).unwrap(),
            "preserved",
        );
        // Stale bundled copy is cleaned up so the next migration is a no-op.
        assert!(!bundled.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_returns_no_bundled_for_plain_binary() {
        let root = unique_temp_root("migrate-noop");
        let data_root = root.join("SnappyRef");
        // Plain-binary layout: executable lives directly under the data root.
        let executable_dir = data_root.clone();
        std::fs::create_dir_all(&data_root).unwrap();

        let outcome = migrate_bundled_model(&data_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::NoBundledModel);
        assert!(!data_root.join("models").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_runs_at_most_once_for_real_workflow() {
        // Simulates "first launch after upgrade" followed by "second launch":
        // the second call must not find a bundled directory and must not touch
        // the migrated store.
        let root = unique_temp_root("migrate-twice");
        let data_root = root.join("SnappyRef");
        let executable_dir = data_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = data_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "{}");

        let first = migrate_bundled_model(&data_root, &executable_dir).unwrap();
        let second = migrate_bundled_model(&data_root, &executable_dir).unwrap();

        assert_eq!(first, MigrationOutcome::Migrated);
        assert_eq!(second, MigrationOutcome::NoBundledModel);
        // Migrated content still present.
        assert!(data_root.join("models/siglip2/tokenizer.json").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_model_user_message_mentions_install_surfaces() {
        assert!(MISSING_MODEL_USER_MESSAGE.contains("Settings"));
        assert!(MISSING_MODEL_USER_MESSAGE.contains("search palette"));
    }
}
