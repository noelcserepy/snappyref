use std::path::{Path, PathBuf};

pub(crate) const REFERENCE_LIBRARY_ROOT_ENV: &str = "REFERENCE_LIBRARY_ROOT";

pub(crate) fn resolve_library_root() -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let install_root = current_exe
        .parent()
        .ok_or_else(|| format!("Executable path has no parent: {}", current_exe.display()))?;
    let override_root = std::env::var_os(REFERENCE_LIBRARY_ROOT_ENV).map(PathBuf::from);

    Ok(library_root_from_parts(install_root, override_root))
}

fn library_root_from_parts(install_root: &Path, override_root: Option<PathBuf>) -> PathBuf {
    override_root.unwrap_or_else(|| install_root.join("library"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_root_wins() {
        let install_root = Path::new("/fake/Reference");
        let override_root = PathBuf::from("/tmp/reference-library");

        let root = library_root_from_parts(install_root, Some(override_root.clone()));

        assert_eq!(root, override_root);
    }

    #[test]
    fn default_root_uses_install_root_library() {
        let install_root = Path::new("/fake/Reference");

        let root = library_root_from_parts(install_root, None);

        assert_eq!(root, PathBuf::from("/fake/Reference/library"));
    }
}
