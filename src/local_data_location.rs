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
    if let Some(root) = std::env::var_os(SNAPPYREF_LIBRARY_ROOT_ENV) {
        return Ok(PathBuf::from(root));
    }
    Ok(resolve_data_root()?.join("library"))
}

pub(crate) fn resolve_data_root() -> Result<PathBuf, String> {
    if let Some(override_root) = std::env::var_os(SNAPPYREF_DATA_ROOT_ENV) {
        return Ok(PathBuf::from(override_root));
    }
    let current_exe = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve current executable path: {error}"))?;
    let install_root = install_root_from_exe_path(&current_exe)?;
    resolve_data_root_from_install_root(&install_root)
}

fn resolve_data_root_from_install_root(install_root: &Path) -> Result<PathBuf, String> {
    if install_root_is_writable(install_root) {
        return Ok(install_root.to_path_buf());
    }
    platform_user_data_root()
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
        home_dir()
            .map(|home| {
                home.join("Library")
                    .join("Application Support")
                    .join("SnappyRef")
            })
            .ok_or_else(|| "Failed to resolve user data directory: HOME is not set".to_owned())
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(root) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            return Ok(root.join("SnappyRef"));
        }
        if let Some(root) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return Ok(root.join("SnappyRef"));
        }
        Err(
            "Failed to resolve user data directory: LOCALAPPDATA and APPDATA are not set"
                .to_owned(),
        )
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
    fn root_resolution_respects_overrides_portable_installs_and_platform_fallback() {
        const CHILD: &str = "SNAPPYREF_ROOT_TEST_CHILD";
        if let Ok(kind) = std::env::var(CHILD) {
            let root = PathBuf::from(std::env::var_os("SNAPPYREF_ROOT_TEST_PATH").unwrap());
            match kind.as_str() {
                "library" => assert_eq!(resolve_library_root().unwrap(), root),
                "data" => {
                    assert_eq!(resolve_data_root().unwrap(), root);
                    assert_eq!(resolve_library_root().unwrap(), root.join("library"));
                }
                "portable" => {
                    assert_eq!(resolve_data_root_from_install_root(&root).unwrap(), root);
                    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
                }
                #[cfg(unix)]
                "fallback" => {
                    use std::os::unix::fs::PermissionsExt as _;
                    let protected = root.join("protected-install");
                    std::fs::create_dir(&protected).unwrap();
                    std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o555))
                        .unwrap();
                    let resolved = resolve_data_root_from_install_root(&protected);
                    std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                    #[cfg(target_os = "macos")]
                    let expected = root.join("Library/Application Support/SnappyRef");
                    #[cfg(not(target_os = "macos"))]
                    let expected = root.join("SnappyRef");
                    assert_eq!(resolved.unwrap(), expected);
                }
                #[cfg(windows)]
                "appdata" => assert_eq!(
                    resolve_data_root_from_install_root(&root.join("missing-install")).unwrap(),
                    root.join("SnappyRef")
                ),
                _ => unreachable!(),
            }
            return;
        }
        let root = crate::test_support::TempRoot::new("root-resolution");
        let mut kinds = vec!["library", "data", "portable"];
        #[cfg(unix)]
        kinds.push("fallback");
        #[cfg(windows)]
        kinds.push("appdata");
        for kind in kinds {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "local_data_location::tests::root_resolution_respects_overrides_portable_installs_and_platform_fallback"])
                .env(CHILD, kind).env("SNAPPYREF_ROOT_TEST_PATH", &*root);
            for name in [
                "HOME",
                "LOCALAPPDATA",
                "APPDATA",
                "XDG_DATA_HOME",
                SNAPPYREF_LIBRARY_ROOT_ENV,
                SNAPPYREF_DATA_ROOT_ENV,
            ] {
                child.env_remove(name);
            }
            if kind == "library" {
                child.env(SNAPPYREF_LIBRARY_ROOT_ENV, &*root);
            } else if kind == "data" {
                child.env(SNAPPYREF_DATA_ROOT_ENV, &*root);
            }
            if kind == "appdata" {
                child.env("APPDATA", &*root);
            }
            if kind == "fallback" {
                child
                    .env("HOME", &*root)
                    .env("LOCALAPPDATA", &*root)
                    .env("XDG_DATA_HOME", &*root);
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{kind} root resolution failed:\n{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
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
