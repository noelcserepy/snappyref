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
//! [`crate::local_data_location::install_root_from_exe_path`] so plain-binary
//! and macOS `.app` bundle layouts share one source of truth.
//! [`SNAPPYREF_INSTALL_ROOT_ENV`] lets a dev or smoke fixture redirect the
//! entire updates tree to a temporary directory without juggling exe paths.
//!
//! Path construction functions take explicit paths so the module is unit
//! testable without touching `std::env::current_exe`.

use std::path::{Path, PathBuf};

use crate::local_data_location::install_root_from_exe_path;

/// Env override consulted by [`resolve_install_root`]. Documented as a dev/test
/// hook only; production builds rely on the exe-derived install root.
pub(crate) const SNAPPYREF_INSTALL_ROOT_ENV: &str = "SNAPPYREF_INSTALL_ROOT";

/// Directory name for the updates bucket, anchored at the install root.
pub(crate) const UPDATES_DIR_NAME: &str = "updates";

/// Resolve `<install_root>` for the running process. The env override
/// wins; otherwise we derive the install root from the executable path
/// using the same rules as `local_data_location::install_root_from_exe_path`.
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

/// Full path of the in-flight download file for `version`. We do not trust
/// the URL's basename because it is publisher-controlled; the version is.
pub(crate) fn download_file_path(install_root: &Path, version: &str) -> PathBuf {
    updates_dir(install_root).join(format!("download-{version}.zip"))
}
