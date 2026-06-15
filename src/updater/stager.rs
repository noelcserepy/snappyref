//! Extract a verified update zip into
//! `<install_root>/updates/staging-{version}/`, then validate that the
//! result looks like a real SnappyRef app folder for the target platform.
//!
//! Each platform zip contains a single top-level `SnappyRef/` folder. On
//! macOS the folder contains
//! `SnappyRef.app/`; on Windows it contains `snappyref.exe`. The
//! validation step here is what stops a corrupted or malicious zip from
//! ever reaching the restart-apply swap step.
//!
//! Security: every zip entry name is checked for `..` segments and for
//! absolute paths before any filesystem write. A traversal attempt
//! aborts staging and unwinds the partially-extracted tree.

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use zip::ZipArchive;

use crate::updater::platform::TargetPlatform;

/// Top-level folder name expected inside every release zip. Keeping this
/// uniform across macOS and Windows preserves the user's "drag the folder
/// anywhere" install model.
pub(crate) const EXPECTED_TOP_LEVEL: &str = "SnappyRef";

/// Sidecar file written next to a staged bundle recording the sha256 of
/// the source zip that produced it. Used at startup to verify a staging
/// directory left behind by a previous session still matches the
/// manifest's expected hash before promoting the update state to `Ready`.
pub(crate) const STAGED_SHA_FILE: &str = ".staged-sha256";

/// Failure modes for [`extract_and_validate`].
#[derive(Debug)]
pub(crate) enum StageError {
    /// The zip file itself is unreadable or malformed.
    InvalidZip(String),
    /// Filesystem error during extraction.
    Io(io::Error),
    /// A zip entry contained `..` or other path-traversal markers.
    PathTraversal { entry: String },
    /// Top-level layout was wrong (multiple roots, missing app payload, etc).
    InvalidLayout(String),
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageError::InvalidZip(error) => write!(f, "invalid zip: {error}"),
            StageError::Io(error) => write!(f, "io: {error}"),
            StageError::PathTraversal { entry } => {
                write!(f, "path traversal in zip entry `{entry}`")
            }
            StageError::InvalidLayout(error) => write!(f, "invalid layout: {error}"),
        }
    }
}

impl std::error::Error for StageError {}

impl From<io::Error> for StageError {
    fn from(error: io::Error) -> Self {
        StageError::Io(error)
    }
}

/// Outcome of a successful staging.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StageOutcome {
    /// `<install_root>/updates/staging-{version}/`.
    pub staging_root: PathBuf,
    /// `<staging_root>/SnappyRef/`, the top-level extracted folder.
    pub bundle_root: PathBuf,
}

/// Write the source-zip sha256 sidecar inside a successfully-staged
/// directory. Best-effort: a failure is non-fatal because the staging
/// itself is structurally valid; we just lose the freshness check the
/// next session would perform.
pub(crate) fn record_source_sha(staging_root: &Path, sha: &str) {
    let _ = fs::write(
        staging_root.join(STAGED_SHA_FILE),
        sha.trim().to_lowercase(),
    );
}

/// Read the source-zip sha256 sidecar from a staging directory. Returns
/// `None` if the sidecar is missing or unreadable.
pub(crate) fn read_source_sha(staging_root: &Path) -> Option<String> {
    fs::read_to_string(staging_root.join(STAGED_SHA_FILE))
        .ok()
        .map(|raw| raw.trim().to_lowercase())
}

/// Extract `zip_path` into `staging_root`, rejecting any entry that
/// resolves outside `staging_root`. On any error the staging directory
/// is removed so we never leave a half-extracted tree on disk.
pub(crate) fn extract_and_validate(
    zip_path: &Path,
    staging_root: &Path,
    platform: TargetPlatform,
) -> Result<StageOutcome, StageError> {
    // Start clean. A previous failed attempt may have left files behind;
    // the verified-then-staged invariant requires this directory either
    // not exist or be entirely the product of this extraction.
    if staging_root.exists() {
        fs::remove_dir_all(staging_root)?;
    }
    fs::create_dir_all(staging_root)?;

    match extract_inner(zip_path, staging_root) {
        Ok(_) => {}
        Err(error) => {
            let _ = fs::remove_dir_all(staging_root);
            return Err(error);
        }
    }

    let bundle_root = staging_root.join(EXPECTED_TOP_LEVEL);
    match validate_layout(&bundle_root, platform) {
        Ok(()) => Ok(StageOutcome {
            staging_root: staging_root.to_path_buf(),
            bundle_root,
        }),
        Err(error) => {
            let _ = fs::remove_dir_all(staging_root);
            Err(error)
        }
    }
}

fn extract_inner(zip_path: &Path, staging_root: &Path) -> Result<(), StageError> {
    let file = File::open(zip_path).map_err(StageError::Io)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| StageError::InvalidZip(error.to_string()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|error| StageError::InvalidZip(error.to_string()))?;
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
        {
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
fn sanitize_entry_path(name: &str) -> Result<PathBuf, StageError> {
    if name.is_empty() {
        return Err(StageError::InvalidLayout(
            "zip contains an empty entry name".to_owned(),
        ));
    }
    // Reject Windows drive prefixes (`C:`) and the back-slash absolute
    // form before parsing as a Path so we don't accidentally accept them
    // on a non-Windows host.
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(StageError::PathTraversal {
            entry: name.to_owned(),
        });
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(StageError::PathTraversal {
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
                return Err(StageError::PathTraversal {
                    entry: name.to_owned(),
                });
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(StageError::InvalidLayout(format!(
            "zip entry `{name}` resolves to no path"
        )));
    }
    Ok(clean)
}

/// Verify the extracted tree matches the platform's expected layout.
fn validate_layout(bundle_root: &Path, platform: TargetPlatform) -> Result<(), StageError> {
    if !bundle_root.is_dir() {
        return Err(StageError::InvalidLayout(format!(
            "expected top-level `{}/` folder inside the zip",
            EXPECTED_TOP_LEVEL
        )));
    }
    match platform {
        TargetPlatform::Macos => {
            // Match the macOS install layout used by library_location:
            //   SnappyRef/SnappyRef.app/Contents/MacOS/{executable}
            // We don't pin the executable filename — slices that build
            // the manifest may rename it — but Contents/MacOS must
            // exist and contain at least one file.
            let app = bundle_root.join("SnappyRef.app");
            if !app.is_dir() {
                return Err(StageError::InvalidLayout(
                    "macOS zip is missing `SnappyRef/SnappyRef.app/`".to_owned(),
                ));
            }
            let macos_dir = app.join("Contents").join("MacOS");
            if !macos_dir.is_dir() {
                return Err(StageError::InvalidLayout(
                    "macOS zip is missing `SnappyRef.app/Contents/MacOS/`".to_owned(),
                ));
            }
            let any_executable = fs::read_dir(&macos_dir)
                .map_err(StageError::Io)?
                .filter_map(Result::ok)
                .any(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false));
            if !any_executable {
                return Err(StageError::InvalidLayout(
                    "macOS zip's `Contents/MacOS/` is empty".to_owned(),
                ));
            }
            let helper = macos_dir.join("snappyref-updater");
            if !helper.is_file() {
                return Err(StageError::InvalidLayout(
                    "macOS zip is missing `SnappyRef.app/Contents/MacOS/snappyref-updater`"
                        .to_owned(),
                ));
            }
        }
        TargetPlatform::Windows => {
            // Windows layout: SnappyRef/snappyref.exe at the folder root.
            let exe = bundle_root.join("snappyref.exe");
            if !exe.is_file() {
                return Err(StageError::InvalidLayout(
                    "Windows zip is missing `SnappyRef/snappyref.exe`".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "snappyref-stager-{prefix}-{}-{}-{nanos}",
            std::process::id(),
            counter,
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options() -> SimpleFileOptions {
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = ZipWriter::new(file);
        for (name, payload) in entries {
            if payload.is_empty() && name.ends_with('/') {
                writer.add_directory(*name, options()).unwrap();
            } else {
                writer.start_file(*name, options()).unwrap();
                writer.write_all(payload).unwrap();
            }
        }
        writer.finish().unwrap();
    }

    fn macos_zip_entries<'a>() -> Vec<(&'a str, &'a [u8])> {
        vec![
            ("SnappyRef/", &[][..]),
            ("SnappyRef/SnappyRef.app/", &[][..]),
            ("SnappyRef/SnappyRef.app/Contents/", &[][..]),
            ("SnappyRef/SnappyRef.app/Contents/MacOS/", &[][..]),
            (
                "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref",
                b"fake-binary",
            ),
            (
                "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref-updater",
                b"fake-helper",
            ),
            ("SnappyRef/SnappyRef.app/Contents/Info.plist", b"<plist/>"),
        ]
    }

    fn windows_zip_entries<'a>() -> Vec<(&'a str, &'a [u8])> {
        vec![
            ("SnappyRef/", &[][..]),
            ("SnappyRef/snappyref.exe", b"fake-exe"),
        ]
    }

    #[test]
    fn extracts_macos_zip_with_expected_layout() {
        let dir = unique_temp_dir("macos-ok");
        let zip_path = dir.join("ok.zip");
        write_zip(&zip_path, &macos_zip_entries());
        let staging = dir.join("staging-1.0.0");

        let outcome = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap();

        assert_eq!(outcome.staging_root, staging);
        assert_eq!(outcome.bundle_root, staging.join("SnappyRef"));
        assert!(
            staging
                .join("SnappyRef/SnappyRef.app/Contents/MacOS/snappyref")
                .is_file()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_windows_zip_with_expected_layout() {
        let dir = unique_temp_dir("windows-ok");
        let zip_path = dir.join("ok.zip");
        write_zip(&zip_path, &windows_zip_entries());
        let staging = dir.join("staging-1.0.0");

        let outcome = extract_and_validate(&zip_path, &staging, TargetPlatform::Windows).unwrap();

        assert_eq!(outcome.bundle_root, staging.join("SnappyRef"));
        assert!(staging.join("SnappyRef/snappyref.exe").is_file());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zip_entry_with_parent_traversal() {
        let dir = unique_temp_dir("traversal");
        let zip_path = dir.join("evil.zip");
        // The malicious entry: a sibling outside SnappyRef.
        write_zip(
            &zip_path,
            &[("SnappyRef/", &[][..]), ("SnappyRef/../escape.txt", b"pwn")],
        );
        let staging = dir.join("staging-evil");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        match err {
            StageError::PathTraversal { entry } => {
                assert!(entry.contains(".."), "entry was {entry}");
            }
            other => panic!("expected PathTraversal, got {other:?}"),
        }
        assert!(
            !staging.exists(),
            "staging dir must be cleaned up after rejection",
        );
        // The escape sibling must not exist on disk.
        assert!(!dir.join("escape.txt").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zip_entry_with_absolute_path() {
        let dir = unique_temp_dir("absolute");
        let zip_path = dir.join("abs.zip");
        write_zip(&zip_path, &[("/etc/passwd", b"nope")]);
        let staging = dir.join("staging-abs");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        assert!(matches!(err, StageError::PathTraversal { .. }), "{err:?}");
        assert!(!staging.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zip_entry_with_windows_drive_prefix() {
        let dir = unique_temp_dir("drive");
        let zip_path = dir.join("drive.zip");
        write_zip(&zip_path, &[("C:/SnappyRef/snappyref.exe", b"nope")]);
        let staging = dir.join("staging-drive");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Windows).unwrap_err();
        assert!(matches!(err, StageError::PathTraversal { .. }), "{err:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zip_entry_with_backslash_parent_traversal() {
        // On a publisher built on Windows, separator may be `\`. The
        // sanitizer must normalize so it can still catch `..`.
        let dir = unique_temp_dir("backslash");
        let zip_path = dir.join("bs.zip");
        write_zip(&zip_path, &[("SnappyRef\\..\\escape.txt", b"nope")]);
        let staging = dir.join("staging-bs");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        assert!(matches!(err, StageError::PathTraversal { .. }), "{err:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_malformed_zip() {
        let dir = unique_temp_dir("malformed");
        let zip_path = dir.join("garbage.zip");
        fs::write(&zip_path, b"this is not a zip file").unwrap();
        let staging = dir.join("staging-malformed");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        assert!(matches!(err, StageError::InvalidZip(_)), "{err:?}");
        assert!(!staging.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_zip_missing_top_level_folder() {
        let dir = unique_temp_dir("no-top");
        let zip_path = dir.join("flat.zip");
        // Note: no `SnappyRef/` prefix; valid archives always have one
        // portable install root.
        write_zip(&zip_path, &[("snappyref.exe", b"fake")]);
        let staging = dir.join("staging-flat");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Windows).unwrap_err();
        match err {
            StageError::InvalidLayout(_) => {}
            other => panic!("expected InvalidLayout, got {other:?}"),
        }
        assert!(!staging.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_macos_zip_missing_app_bundle() {
        let dir = unique_temp_dir("no-app");
        let zip_path = dir.join("no-app.zip");
        write_zip(
            &zip_path,
            &[
                ("SnappyRef/", &[][..]),
                ("SnappyRef/snappyref", b"fake-binary"),
            ],
        );
        let staging = dir.join("staging-no-app");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        match err {
            StageError::InvalidLayout(msg) => {
                assert!(msg.contains("SnappyRef.app"), "msg was {msg}");
            }
            other => panic!("expected InvalidLayout, got {other:?}"),
        }
        assert!(!staging.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_windows_zip_missing_exe() {
        let dir = unique_temp_dir("no-exe");
        let zip_path = dir.join("no-exe.zip");
        write_zip(
            &zip_path,
            &[("SnappyRef/", &[][..]), ("SnappyRef/README.txt", b"hello")],
        );
        let staging = dir.join("staging-no-exe");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Windows).unwrap_err();
        match err {
            StageError::InvalidLayout(msg) => {
                assert!(msg.contains("snappyref.exe"), "msg was {msg}");
            }
            other => panic!("expected InvalidLayout, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_macos_zip_missing_updater_helper() {
        let dir = unique_temp_dir("macos-no-helper");
        let zip_path = dir.join("no-helper.zip");
        write_zip(
            &zip_path,
            &[
                ("SnappyRef/", &[][..]),
                ("SnappyRef/SnappyRef.app/", &[][..]),
                ("SnappyRef/SnappyRef.app/Contents/", &[][..]),
                ("SnappyRef/SnappyRef.app/Contents/MacOS/", &[][..]),
                (
                    "SnappyRef/SnappyRef.app/Contents/MacOS/snappyref",
                    b"fake-binary",
                ),
                ("SnappyRef/SnappyRef.app/Contents/Info.plist", b"<plist/>"),
            ],
        );
        let staging = dir.join("staging-no-helper");

        let err = extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap_err();
        match err {
            StageError::InvalidLayout(msg) => {
                assert!(msg.contains("snappyref-updater"), "msg was {msg}");
            }
            other => panic!("expected InvalidLayout, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_replaces_existing_staging_dir() {
        let dir = unique_temp_dir("replace");
        let zip_path = dir.join("ok.zip");
        write_zip(&zip_path, &macos_zip_entries());
        let staging = dir.join("staging-1.0.0");
        // Stale prior content that must be removed before the new
        // extraction. We pick a file that the new zip does not write.
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("stale-file.txt"), b"old").unwrap();

        extract_and_validate(&zip_path, &staging, TargetPlatform::Macos).unwrap();

        assert!(
            !staging.join("stale-file.txt").exists(),
            "previous staging contents must be wiped before re-extract",
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
