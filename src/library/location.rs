use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// Root vocabulary:
// - install root: where the portable app lives.
// - data root: writable app-owned state; usually the install root, falling
//   back to the platform user-data folder for protected installs.
// - library root: the managed Library folder under the data root unless tests
//   or tools explicitly override it.
pub(crate) const SNAPPYREF_LIBRARY_ROOT_ENV: &str = "SNAPPYREF_LIBRARY_ROOT";
pub(crate) const SNAPPYREF_DATA_ROOT_ENV: &str = "SNAPPYREF_DATA_ROOT";

pub(crate) fn resolve_library_root() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let install_root = install_root_from_exe_path(&current_exe)?;
    let override_root = std::env::var_os(SNAPPYREF_LIBRARY_ROOT_ENV).map(PathBuf::from);

    Ok(library_root_from_parts(
        override_root,
        resolve_data_root_from_install_root(&install_root)?,
    ))
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
pub(crate) fn resolve_data_root() -> Result<PathBuf, String> {
    if let Some(override_root) = std::env::var_os(SNAPPYREF_DATA_ROOT_ENV) {
        return Ok(PathBuf::from(override_root));
    }
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let install_root = install_root_from_exe_path(&current_exe)?;
    resolve_data_root_from_install_root(&install_root)
}

fn library_root_from_parts(override_root: Option<PathBuf>, data_root: PathBuf) -> PathBuf {
    override_root.unwrap_or_else(|| data_root.join("library"))
}

fn resolve_data_root_from_install_root(install_root: &Path) -> Result<PathBuf, String> {
    if let Some(override_root) = std::env::var_os(SNAPPYREF_DATA_ROOT_ENV) {
        return Ok(PathBuf::from(override_root));
    }
    Ok(data_root_from_parts(
        install_root,
        install_root_is_writable(install_root),
        platform_user_data_root()?,
    ))
}

fn data_root_from_parts(
    install_root: &Path,
    install_root_writable: bool,
    user_data_root: PathBuf,
) -> PathBuf {
    if install_root_writable {
        install_root.to_path_buf()
    } else {
        user_data_root
    }
}

fn install_root_is_writable(install_root: &Path) -> bool {
    if !install_root.is_dir() {
        return false;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let probe = install_root.join(format!(
        ".snappyref-write-test-{}-{nanos}",
        std::process::id()
    ));
    let writable = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if writable {
        let _ = std::fs::remove_file(probe);
    }
    writable
}

fn platform_user_data_root() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        return home_dir()
            .map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join("SnappyRef")
            })
            .ok_or_else(|| "Failed to resolve user data directory: HOME is not set".to_owned());
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(root) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            return Ok(root.join("SnappyRef"));
        }
        if let Some(root) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return Ok(root.join("SnappyRef"));
        }
        return Err(
            "Failed to resolve user data directory: LOCALAPPDATA and APPDATA are not set"
                .to_owned(),
        );
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        if let Some(root) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
            return Ok(root.join("SnappyRef"));
        }
        return home_dir()
            .map(|home| home.join(".local").join("share").join("SnappyRef"))
            .ok_or_else(|| "Failed to resolve user data directory: HOME is not set".to_owned());
    }
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub(crate) fn install_root_from_exe_path(current_exe: &Path) -> Result<PathBuf, String> {
    let executable_dir = current_exe
        .parent()
        .ok_or_else(|| format!("Executable path has no parent: {}", current_exe.display()))?;

    Ok(macos_app_install_root(executable_dir).unwrap_or_else(|| executable_dir.to_path_buf()))
}

fn macos_app_install_root(executable_dir: &Path) -> Option<PathBuf> {
    let contents_dir = executable_dir.parent()?;
    if executable_dir.file_name()? != "MacOS" || contents_dir.file_name()? != "Contents" {
        return None;
    }

    let app_dir = contents_dir.parent()?;
    if app_dir.extension()? != "app" {
        return None;
    }

    app_dir.parent().map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_root_wins() {
        let override_root = PathBuf::from("/tmp/snappyref-library");
        let data_root = PathBuf::from("/tmp/snappyref-data");

        let root = library_root_from_parts(Some(override_root.clone()), data_root);

        assert_eq!(root, override_root);
    }

    #[test]
    fn default_root_uses_install_root_library() {
        let data_root = PathBuf::from("/fake/SnappyRef");

        let root = library_root_from_parts(None, data_root);

        assert_eq!(root, PathBuf::from("/fake/SnappyRef/library"));
    }

    #[test]
    fn data_root_uses_install_root_when_writable() {
        let install_root = Path::new("/fake/SnappyRef");
        let user_data_root = PathBuf::from("/Users/alice/Library/Application Support/SnappyRef");

        let root = data_root_from_parts(install_root, true, user_data_root);

        assert_eq!(root, install_root);
    }

    #[test]
    fn data_root_falls_back_to_user_data_when_install_root_is_not_writable() {
        let install_root = Path::new("/Applications");
        let user_data_root = PathBuf::from("/Users/alice/Library/Application Support/SnappyRef");

        let root = data_root_from_parts(install_root, false, user_data_root.clone());

        assert_eq!(root, user_data_root);
    }

    #[test]
    fn install_root_uses_executable_parent_for_plain_binary() {
        let exe = Path::new("/fake/SnappyRef/snappyref");

        let root = install_root_from_exe_path(exe).unwrap();

        assert_eq!(root, PathBuf::from("/fake/SnappyRef"));
    }

    #[test]
    fn install_root_uses_app_parent_for_macos_app_bundle() {
        let exe = Path::new("/fake/SnappyRef/SnappyRef.app/Contents/MacOS/snappyref");

        let root = install_root_from_exe_path(exe).unwrap();

        assert_eq!(root, PathBuf::from("/fake/SnappyRef"));
    }
}
