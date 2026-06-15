//! Install-root–relative paths for auto-update download/staging.
//!
//! Auto-update state stays inside the user's portable install folder. The
//! updater uses two locations relative to the install root:
//!
//! * `<install_root>/updates/` — the download bucket. Partial downloads,
//!   verified zips, and staging subdirectories all live here.
//! * `<install_root>/updates/staging-{version}/` — the extracted, verified
//!   bundle for a specific manifest version, used by the restart-apply step.
//!
//! The install-root resolution itself reuses
//! [`crate::library::location::install_root_from_exe_path`] so plain-binary
//! and macOS `.app` bundle layouts share one source of truth. Tests pin
//! the env-override case via [`SNAPPYREF_INSTALL_ROOT_ENV`] which lets a
//! dev or test fixture redirect the entire updates tree to a temporary
//! directory without juggling exe paths.
//!
//! Every public function takes explicit paths so the module is unit
//! testable without touching `std::env::current_exe`.

use std::path::{Path, PathBuf};

use crate::library::location::install_root_from_exe_path;

/// Env override consulted by [`resolve_install_root`]. Documented as a dev/test
/// hook only; production builds rely on the exe-derived install root.
pub(crate) const SNAPPYREF_INSTALL_ROOT_ENV: &str = "SNAPPYREF_INSTALL_ROOT";

/// Directory name for the updates bucket, anchored at the install root.
pub(crate) const UPDATES_DIR_NAME: &str = "updates";

/// Resolve `<install_root>` for the running process. The env override
/// wins; otherwise we derive the install root from the executable path
/// using the same rules as `library_location::resolve_install_root`.
pub(crate) fn resolve_install_root() -> Result<PathBuf, String> {
    if let Some(override_root) = std::env::var_os(SNAPPYREF_INSTALL_ROOT_ENV) {
        return Ok(PathBuf::from(override_root));
    }
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    install_root_from_exe_path(&current_exe)
}

/// `<install_root>/updates/`, the bucket that holds in-flight downloads,
/// verified zips, and staging subdirectories for each candidate version.
pub(crate) fn updates_dir(install_root: &Path) -> PathBuf {
    install_root.join(UPDATES_DIR_NAME)
}

/// `<install_root>/updates/staging-{version}/`, the extracted-and-verified
/// bundle root for a specific manifest version.
pub(crate) fn staging_dir(install_root: &Path, version: &str) -> PathBuf {
    updates_dir(install_root).join(format!("staging-{version}"))
}

/// Conservative download filename for the manifest's zip. We do not trust
/// the URL's basename because it is publisher-controlled; the version is.
pub(crate) fn download_file_name(version: &str) -> String {
    format!("download-{version}.zip")
}

/// Full path of the in-flight download file for `version`.
pub(crate) fn download_file_path(install_root: &Path, version: &str) -> PathBuf {
    updates_dir(install_root).join(download_file_name(version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_dir_is_install_root_slash_updates_for_plain_binary() {
        let install_root = Path::new("/fake/SnappyRef");
        assert_eq!(
            updates_dir(install_root),
            PathBuf::from("/fake/SnappyRef/updates"),
        );
    }

    #[test]
    fn updates_dir_is_outside_app_bundle_for_macos_layout() {
        // On macOS the install root is the *parent* of SnappyRef.app, so
        // `<install_root>/updates/` is a sibling of the app bundle just
        // like `library/` and `models/`, so the swap step writes inside
        // the user's portable folder without descending into the bundle.
        let install_root = Path::new("/Users/alice/SnappyRef");
        assert_eq!(
            updates_dir(install_root),
            PathBuf::from("/Users/alice/SnappyRef/updates"),
        );
    }

    #[test]
    fn staging_dir_is_versioned_subdir_of_updates() {
        let install_root = Path::new("/fake/SnappyRef");
        assert_eq!(
            staging_dir(install_root, "1.2.3"),
            PathBuf::from("/fake/SnappyRef/updates/staging-1.2.3"),
        );
    }

    #[test]
    fn staging_dir_versions_are_isolated_per_version_string() {
        let install_root = Path::new("/fake/SnappyRef");
        let a = staging_dir(install_root, "1.2.3");
        let b = staging_dir(install_root, "1.2.4");
        assert_ne!(a, b);
    }

    #[test]
    fn download_file_path_lives_directly_under_updates_dir() {
        let install_root = Path::new("/fake/SnappyRef");
        let file = download_file_path(install_root, "1.2.3");
        assert_eq!(file.parent().unwrap(), updates_dir(install_root));
        assert!(
            file.file_name()
                .and_then(|n| n.to_str())
                .unwrap()
                .ends_with(".zip")
        );
    }

    #[test]
    fn resolve_install_root_honors_env_override() {
        // Pin a unique env value so concurrent test threads do not race.
        let scratch = std::env::temp_dir().join(format!(
            "snappyref-install-root-test-{}",
            std::process::id()
        ));
        // Safety: env mutation is process-global. We use a unique value and
        // restore the prior state to avoid leaking into other tests; tests
        // touching this env in the same process must be serial. The cargo
        // test harness does not parallelize across processes here, and no
        // other test in this module reads the env.
        let prior = std::env::var_os(SNAPPYREF_INSTALL_ROOT_ENV);
        // SAFETY: see comment above; restored before the test returns.
        unsafe { std::env::set_var(SNAPPYREF_INSTALL_ROOT_ENV, &scratch) };
        let resolved = resolve_install_root().unwrap();
        // SAFETY: restore prior env state.
        unsafe {
            match prior {
                Some(value) => std::env::set_var(SNAPPYREF_INSTALL_ROOT_ENV, value),
                None => std::env::remove_var(SNAPPYREF_INSTALL_ROOT_ENV),
            }
        }
        assert_eq!(resolved, scratch);
    }
}
