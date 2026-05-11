//! Local Model Store: resolves and migrates the on-disk model directory that
//! lives outside the app bundle at `<install_root>/models/`.
//!
//! This module is the single source of truth for:
//! - resolving `<install_root>/models/` across macOS `.app` and plain-binary layouts
//! - detecting an in-bundle model directory left over from older installs
//! - performing a one-shot, idempotent migration that moves the in-bundle model
//!   into the Local Model Store
//! - reporting a clear, user-facing error when no model is available
//!
//! The interface is unit-testable: every function that touches the filesystem
//! takes explicit paths so tests can simulate macOS and Windows layouts without
//! relying on `std::env::current_exe`.

use std::path::{Path, PathBuf};

/// The on-disk root of the Local Model Store relative to a given install root.
pub(crate) fn model_store_root(install_root: &Path) -> PathBuf {
    install_root.join("models")
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

/// Status of model availability for a given install root + executable layout.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModelAvailability {
    /// A Local Model Store exists at `<install_root>/models/`.
    LocalStore,
    /// No Local Model Store exists, but an in-bundle model dir is present and
    /// can be migrated.
    BundledOnly,
    /// Neither a Local Model Store nor a bundled model is available.
    Missing,
}

/// Inspect the filesystem and classify model availability.
pub(crate) fn classify_availability(
    install_root: &Path,
    executable_dir: &Path,
) -> ModelAvailability {
    let store = model_store_root(install_root);
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

/// Move the in-bundle `models/` directory (if any) to `<install_root>/models/`.
///
/// This is idempotent: if `<install_root>/models/` already has content it is
/// left alone and the in-bundle directory is removed. If no in-bundle model
/// exists, the function returns `NoBundledModel`.
pub(crate) fn migrate_bundled_model(
    install_root: &Path,
    executable_dir: &Path,
) -> Result<MigrationOutcome, String> {
    let Some(bundled) = bundled_model_dir(executable_dir) else {
        return Ok(MigrationOutcome::NoBundledModel);
    };
    if !dir_has_entries(&bundled) {
        return Ok(MigrationOutcome::NoBundledModel);
    }

    let store = model_store_root(install_root);
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
/// anywhere. Points the user at the full-install download.
pub(crate) const MISSING_MODEL_USER_MESSAGE: &str = "Semantic search model not found. Download the full SnappyRef install zip \
     (the one that includes the models folder) and place SnappyRef next to a \
     models/ folder before launching.";

fn dir_has_entries(path: &Path) -> bool {
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.next().is_some()
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

    #[test]
    fn model_store_root_is_sibling_of_library_for_plain_binary() {
        let install_root = PathBuf::from("/fake/SnappyRef");
        assert_eq!(
            model_store_root(&install_root),
            PathBuf::from("/fake/SnappyRef/models"),
        );
    }

    #[test]
    fn model_store_root_for_macos_install_root_is_outside_app_bundle() {
        // On macOS the install root is the parent of SnappyRef.app — same shape
        // as the plain-binary case, so `<install_root>/models/` is a sibling of
        // SnappyRef.app, not nested inside it.
        let install_root = PathBuf::from("/Users/alice/SnappyRef");
        assert_eq!(
            model_store_root(&install_root),
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
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.clone();
        write_file(&install_root.join("models").join("marker.bin"), "data");

        assert_eq!(
            classify_availability(&install_root, &executable_dir),
            ModelAvailability::LocalStore,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_availability_reports_bundled_only_for_macos_app_layout() {
        let root = unique_temp_root("availability-bundled");
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = install_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/marker.bin"), "data");

        assert_eq!(
            classify_availability(&install_root, &executable_dir),
            ModelAvailability::BundledOnly,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classify_availability_reports_missing_when_nothing_present() {
        let root = unique_temp_root("availability-missing");
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();

        assert_eq!(
            classify_availability(&install_root, &executable_dir),
            ModelAvailability::Missing,
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_moves_files_to_local_store_once() {
        let root = unique_temp_root("migrate-once");
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = install_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "{}");
        write_file(&bundled.join("siglip2/onnx/text_model.onnx"), "weights");

        let outcome = migrate_bundled_model(&install_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::Migrated);
        let store = install_root.join("models");
        assert!(store.join("siglip2/tokenizer.json").exists());
        assert!(store.join("siglip2/onnx/text_model.onnx").exists());
        // Source must be gone after a successful migration.
        assert!(!bundled.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_is_idempotent_when_store_already_present() {
        let root = unique_temp_root("migrate-idempotent");
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();

        // Pre-existing Local Model Store with a known marker.
        write_file(
            &install_root.join("models/siglip2/tokenizer.json"),
            "preserved",
        );
        // Stale in-bundle copy with different contents.
        let bundled = install_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "stale");

        let outcome = migrate_bundled_model(&install_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::AlreadyMigrated);
        // Local Model Store contents are preserved (not overwritten).
        assert_eq!(
            std::fs::read_to_string(install_root.join("models/siglip2/tokenizer.json")).unwrap(),
            "preserved",
        );
        // Stale bundled copy is cleaned up so the next migration is a no-op.
        assert!(!bundled.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_returns_no_bundled_for_plain_binary() {
        let root = unique_temp_root("migrate-noop");
        let install_root = root.join("SnappyRef");
        // Plain-binary layout: executable lives directly under the install root.
        let executable_dir = install_root.clone();
        std::fs::create_dir_all(&install_root).unwrap();

        let outcome = migrate_bundled_model(&install_root, &executable_dir).unwrap();

        assert_eq!(outcome, MigrationOutcome::NoBundledModel);
        assert!(!install_root.join("models").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_bundled_model_runs_at_most_once_for_real_workflow() {
        // Simulates "first launch after upgrade" followed by "second launch":
        // the second call must not find a bundled directory and must not touch
        // the migrated store.
        let root = unique_temp_root("migrate-twice");
        let install_root = root.join("SnappyRef");
        let executable_dir = install_root.join("SnappyRef.app/Contents/MacOS");
        std::fs::create_dir_all(&executable_dir).unwrap();
        let bundled = install_root.join("SnappyRef.app/Contents/Resources/models");
        write_file(&bundled.join("siglip2/tokenizer.json"), "{}");

        let first = migrate_bundled_model(&install_root, &executable_dir).unwrap();
        let second = migrate_bundled_model(&install_root, &executable_dir).unwrap();

        assert_eq!(first, MigrationOutcome::Migrated);
        assert_eq!(second, MigrationOutcome::NoBundledModel);
        // Migrated content still present.
        assert!(install_root.join("models/siglip2/tokenizer.json").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_model_user_message_mentions_full_install() {
        // The message must point users at the full-install download so a fresh
        // install of the app-only zip is recoverable without a developer ticket.
        assert!(MISSING_MODEL_USER_MESSAGE.contains("full"));
        assert!(MISSING_MODEL_USER_MESSAGE.contains("models"));
    }
}
