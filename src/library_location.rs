use std::path::{Path, PathBuf};

pub(crate) const SNAPPYREF_LIBRARY_ROOT_ENV: &str = "SNAPPYREF_LIBRARY_ROOT";

pub(crate) fn resolve_library_root() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let install_root = install_root_from_exe_path(&current_exe)?;
    let override_root = std::env::var_os(SNAPPYREF_LIBRARY_ROOT_ENV).map(PathBuf::from);

    Ok(library_root_from_parts(&install_root, override_root))
}

#[cfg_attr(not(feature = "semantic-search"), allow(dead_code))]
pub(crate) fn resolve_install_root() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    install_root_from_exe_path(&current_exe)
}

fn library_root_from_parts(install_root: &Path, override_root: Option<PathBuf>) -> PathBuf {
    override_root.unwrap_or_else(|| install_root.join("library"))
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
        let install_root = Path::new("/fake/SnappyRef");
        let override_root = PathBuf::from("/tmp/snappyref-library");

        let root = library_root_from_parts(install_root, Some(override_root.clone()));

        assert_eq!(root, override_root);
    }

    #[test]
    fn default_root_uses_install_root_library() {
        let install_root = Path::new("/fake/SnappyRef");

        let root = library_root_from_parts(install_root, None);

        assert_eq!(root, PathBuf::from("/fake/SnappyRef/library"));
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
