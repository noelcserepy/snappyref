//! Pending-update descriptor exchanged between the running app and the
//! `snappyref-updater` helper binary.
//!
//! The app writes this descriptor as JSON into `<install_root>/updates/`
//! right before it exits and hands the path to the helper as argv[1]. The
//! helper reads it, waits for the app's PID to exit, swaps the bundle in
//! place, and relaunches the new version.
//!
//! Layout decisions (ADR-0015):
//!
//! * Every path is absolute so the helper does not need to inherit the
//!   app's working directory.
//! * `current_bundle` and `staged_bundle` are the *swap units* — on macOS
//!   that's `<install_root>/SnappyRef.app` and
//!   `<staging>/SnappyRef/SnappyRef.app`; on Windows it's
//!   `<install_root>/snappyref.exe` and `<staging>/SnappyRef/snappyref.exe`.
//! * `staging_dir` is `<install_root>/updates/staging-{version}/` so the
//!   helper can remove the staging tree after a successful relaunch.
//! * `launch_args` carries the original argv tail (everything after argv[0])
//!   so the helper can re-invoke the app with the same flags the user
//!   started it with.
//! * `expected_new_version` is recorded so post-mortem inspection of an
//!   abandoned descriptor explains what swap was intended.
//! * `parent_pid` is the PID the helper waits on before it touches the
//!   bundle. The app's PID at descriptor-write time is captured.
//!
//! Malformed input is rejected up front: missing fields, non-UTF-8 bytes,
//! and unknown fields all surface as a [`DescriptorError`]. The helper
//! refuses to act on anything other than a fully-valid descriptor.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// On-disk filename of the descriptor inside `<install_root>/updates/`.
pub const DESCRIPTOR_FILE_NAME: &str = "pending-update.json";

/// Serializable descriptor handed from the running app to the helper.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingUpdateDescriptor {
    /// PID of the app process the helper must wait on before swapping.
    pub parent_pid: u32,
    /// Version string of the staged update (matches the manifest entry).
    pub expected_new_version: String,
    /// Absolute path to the install root, parent of `library/`, `models/`,
    /// and `updates/`.
    pub install_root: PathBuf,
    /// Absolute path to the *swap unit* that will be moved aside:
    /// `<install_root>/SnappyRef.app` on macOS,
    /// `<install_root>/snappyref.exe` on Windows.
    pub current_bundle: PathBuf,
    /// Absolute path to the *swap unit* inside the staging tree. The helper
    /// renames this onto `current_bundle`'s slot once the parent has exited.
    pub staged_bundle: PathBuf,
    /// Absolute path to the staging directory the helper must remove after
    /// the swap and relaunch succeed:
    /// `<install_root>/updates/staging-{version}/`.
    pub staging_dir: PathBuf,
    /// Original launch args the helper should replay on the new binary
    /// (everything after argv[0]).
    pub launch_args: Vec<String>,
}

/// Failure modes for [`read`]/[`write`].
#[derive(Debug)]
pub enum DescriptorError {
    /// Underlying filesystem error.
    Io(std::io::Error),
    /// JSON parse or schema validation error.
    Schema(String),
}

impl std::fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorError::Io(error) => write!(f, "io: {error}"),
            DescriptorError::Schema(error) => write!(f, "descriptor schema: {error}"),
        }
    }
}

impl std::error::Error for DescriptorError {}

impl From<std::io::Error> for DescriptorError {
    fn from(error: std::io::Error) -> Self {
        DescriptorError::Io(error)
    }
}

/// Default path the app writes the descriptor to:
/// `<install_root>/updates/pending-update.json`.
pub fn default_path(install_root: &Path) -> PathBuf {
    install_root
        .join(crate::updater::install_paths::UPDATES_DIR_NAME)
        .join(DESCRIPTOR_FILE_NAME)
}

/// Serialize `descriptor` as pretty JSON and write it to `path`. Parent
/// directories are created if missing so the caller doesn't need a separate
/// `mkdir`.
pub fn write(path: &Path, descriptor: &PendingUpdateDescriptor) -> Result<(), DescriptorError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(descriptor)
        .map_err(|error| DescriptorError::Schema(error.to_string()))?;
    fs::write(path, bytes)?;
    Ok(())
}

/// Read and validate a descriptor at `path`. Rejects unknown fields,
/// missing required fields, and any non-UTF-8 bytes.
pub fn read(path: &Path) -> Result<PendingUpdateDescriptor, DescriptorError> {
    let bytes = fs::read(path)?;
    parse(&bytes)
}

/// Parse `bytes` as a descriptor. Public for tests and for the helper to
/// re-validate after read.
pub fn parse(bytes: &[u8]) -> Result<PendingUpdateDescriptor, DescriptorError> {
    let descriptor: PendingUpdateDescriptor = serde_json::from_slice(bytes)
        .map_err(|error| DescriptorError::Schema(error.to_string()))?;
    validate(&descriptor)?;
    Ok(descriptor)
}

fn validate(descriptor: &PendingUpdateDescriptor) -> Result<(), DescriptorError> {
    if descriptor.expected_new_version.trim().is_empty() {
        return Err(DescriptorError::Schema(
            "expected_new_version must not be empty".to_owned(),
        ));
    }
    for (name, path) in [
        ("install_root", &descriptor.install_root),
        ("current_bundle", &descriptor.current_bundle),
        ("staged_bundle", &descriptor.staged_bundle),
        ("staging_dir", &descriptor.staging_dir),
    ] {
        if !path.is_absolute() {
            return Err(DescriptorError::Schema(format!(
                "{name} must be an absolute path, got `{}`",
                path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> PendingUpdateDescriptor {
        PendingUpdateDescriptor {
            parent_pid: 4242,
            expected_new_version: "1.2.3".to_owned(),
            install_root: PathBuf::from("/tmp/snappyref"),
            current_bundle: PathBuf::from("/tmp/snappyref/SnappyRef.app"),
            staged_bundle: PathBuf::from(
                "/tmp/snappyref/updates/staging-1.2.3/SnappyRef/SnappyRef.app",
            ),
            staging_dir: PathBuf::from("/tmp/snappyref/updates/staging-1.2.3"),
            launch_args: vec!["--foo".to_owned(), "bar".to_owned()],
        }
    }

    #[test]
    fn round_trip_preserves_every_field() {
        let original = descriptor();
        let bytes = serde_json::to_vec(&original).unwrap();
        let parsed = parse(&bytes).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn write_then_read_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "snappyref-descriptor-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pending.json");
        let original = descriptor();
        write(&path, &original).unwrap();
        let parsed = read(&path).unwrap();
        assert_eq!(parsed, original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_creates_missing_parent_directories() {
        let dir = std::env::temp_dir().join(format!(
            "snappyref-descriptor-nested-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let path = dir.join("nested/dirs/pending.json");
        write(&path, &descriptor()).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_rejects_garbage_bytes() {
        let err = parse(b"not json").unwrap_err();
        assert!(matches!(err, DescriptorError::Schema(_)));
    }

    #[test]
    fn parse_rejects_missing_required_field() {
        // Drop `parent_pid` from an otherwise-valid object. The schema
        // must reject so the helper never reads a half-populated record.
        let bytes = br#"{
            "expected_new_version": "1.0.0",
            "install_root": "/tmp/x",
            "current_bundle": "/tmp/x/SnappyRef.app",
            "staged_bundle": "/tmp/x/updates/staging-1.0.0/SnappyRef/SnappyRef.app",
            "staging_dir": "/tmp/x/updates/staging-1.0.0",
            "launch_args": []
        }"#;
        let err = parse(bytes).unwrap_err();
        assert!(matches!(err, DescriptorError::Schema(_)));
    }

    #[test]
    fn parse_rejects_unknown_field() {
        // `deny_unknown_fields` blocks accidental schema drift between
        // publisher and consumer.
        let bytes = br#"{
            "parent_pid": 1,
            "expected_new_version": "1.0.0",
            "install_root": "/tmp/x",
            "current_bundle": "/tmp/x/SnappyRef.app",
            "staged_bundle": "/tmp/x/updates/staging-1.0.0/SnappyRef/SnappyRef.app",
            "staging_dir": "/tmp/x/updates/staging-1.0.0",
            "launch_args": [],
            "rogue_field": true
        }"#;
        let err = parse(bytes).unwrap_err();
        assert!(matches!(err, DescriptorError::Schema(_)));
    }

    #[test]
    fn parse_rejects_relative_paths() {
        let bytes = br#"{
            "parent_pid": 1,
            "expected_new_version": "1.0.0",
            "install_root": "snappyref",
            "current_bundle": "snappyref/SnappyRef.app",
            "staged_bundle": "snappyref/updates/staging-1.0.0/SnappyRef/SnappyRef.app",
            "staging_dir": "snappyref/updates/staging-1.0.0",
            "launch_args": []
        }"#;
        let err = parse(bytes).unwrap_err();
        match err {
            DescriptorError::Schema(msg) => assert!(msg.contains("absolute"), "msg was {msg}"),
            other => panic!("expected Schema, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_blank_version_string() {
        let bytes = br#"{
            "parent_pid": 1,
            "expected_new_version": "   ",
            "install_root": "/tmp/x",
            "current_bundle": "/tmp/x/SnappyRef.app",
            "staged_bundle": "/tmp/x/updates/staging-1.0.0/SnappyRef/SnappyRef.app",
            "staging_dir": "/tmp/x/updates/staging-1.0.0",
            "launch_args": []
        }"#;
        let err = parse(bytes).unwrap_err();
        match err {
            DescriptorError::Schema(msg) => {
                assert!(msg.contains("expected_new_version"), "msg was {msg}");
            }
            other => panic!("expected Schema, got {other:?}"),
        }
    }

    #[test]
    fn default_path_lives_inside_updates_dir() {
        let install_root = Path::new("/fake/SnappyRef");
        assert_eq!(
            default_path(install_root),
            PathBuf::from("/fake/SnappyRef/updates/pending-update.json"),
        );
    }
}
