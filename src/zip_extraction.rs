//! Contained ZIP extraction shared by app and model staging. Callers own
//! destination cleanup, package layout, and promotion.
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

#[derive(Debug)]
pub(crate) enum ExtractError {
    InvalidZip(String),
    Io(io::Error),
    PathTraversal { entry: String },
    InvalidLayout(String),
}
impl From<io::Error> for ExtractError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
pub(crate) fn extract(
    zip_path: &Path,
    staging_root: &Path,
    preserve_permissions: bool,
) -> Result<(), ExtractError> {
    #[cfg(not(unix))]
    let _ = preserve_permissions;
    let file = File::open(zip_path).map_err(ExtractError::Io)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ExtractError::InvalidZip(error.to_string()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|error| ExtractError::InvalidZip(error.to_string()))?;
        let raw_name = entry.name().to_owned();
        let safe_relative = sanitize_entry_path(&raw_name)?;
        let out_path = staging_root.join(&safe_relative);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = File::create(&out_path)?;
        io::copy(&mut entry, &mut out_file)?;

        #[cfg(unix)]
        if preserve_permissions {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode));
            }
        }
    }

    Ok(())
}

/// Reject entries whose name contains `..`, an absolute path, a Windows
/// drive prefix, or any other component that would escape the staging
/// root. Returns the cleaned `Path` (forward-slash-normalized).
fn sanitize_entry_path(name: &str) -> Result<PathBuf, ExtractError> {
    if name.is_empty() {
        return Err(ExtractError::InvalidLayout(
            "zip contains an empty entry name".to_owned(),
        ));
    }
    // Reject Windows drive prefixes (`C:`) and the back-slash absolute
    // form before parsing as a Path so we don't accidentally accept them
    // on a non-Windows host.
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(ExtractError::PathTraversal {
            entry: name.to_owned(),
        });
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(ExtractError::PathTraversal {
            entry: name.to_owned(),
        });
    }

    // Treat both kinds of separator as path separators so a back-slash
    // crafted on a Windows-style zip cannot evade Component matching on
    // a Unix host.
    let normalized = name.replace('\\', "/");
    let mut clean = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => continue,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ExtractError::PathTraversal {
                    entry: name.to_owned(),
                });
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(ExtractError::InvalidLayout(format!(
            "zip entry `{name}` resolves to no path"
        )));
    }
    Ok(clean)
}
