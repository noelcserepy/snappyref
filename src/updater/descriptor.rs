//! Pending-update descriptor for the macOS `snappyref-updater` helper and
//! the staged Windows executable's self-replacement runner.
//!
//! Before restart, the app writes JSON into `<install_root>/updates/`.
//! The macOS helper receives its path as argv[1]; the Windows runner receives
//! it through self-replacement arguments. Both wait for the old process,
//! replace the app, and relaunch with the original arguments.
//!
//! All paths are absolute. The replacement unit is `SnappyRef.app` on macOS
//! and `snappyref.exe` on Windows. The staging directory is
//! `<install_root>/updates/staging-{version}/` and is removed after relaunch.
//! The expected version records which replacement an abandoned descriptor
//! intended. Missing fields, unknown fields, and invalid JSON are rejected
//! before replacement through [`DescriptorError`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// On-disk filename of the descriptor inside `<install_root>/updates/`.
pub const DESCRIPTOR_FILE_NAME: &str = "pending-update.json";

/// Serializable descriptor handed from the running app to the replacement process.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingUpdateDescriptor {
    /// PID of the app process the replacement process must wait on before swapping.
    pub parent_pid: u32,
    /// Version string of the staged update (matches the manifest entry).
    pub expected_new_version: String,
    /// Absolute install root, which owns `updates/`. Library and model data
    /// may live under a separate writable data root.
    pub install_root: PathBuf,
    /// Absolute path to the *swap unit* that will be moved aside:
    /// `<install_root>/SnappyRef.app` on macOS,
    /// `<install_root>/snappyref.exe` on Windows.
    pub current_bundle: PathBuf,
    /// Absolute staged replacement path, moved to `current_bundle` after
    /// the parent exits.
    pub staged_bundle: PathBuf,
    /// Absolute staging directory, removed after replacement and relaunch:
    /// `<install_root>/updates/staging-{version}/`.
    pub staging_dir: PathBuf,
    /// Original launch arguments after argv[0], replayed on the new binary.
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

/// Parse and validate the descriptor payload.
fn parse(bytes: &[u8]) -> Result<PendingUpdateDescriptor, DescriptorError> {
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

    fn descriptor_root() -> PathBuf {
        std::env::temp_dir().join("snappyref")
    }

    fn descriptor() -> PendingUpdateDescriptor {
        let root = descriptor_root();
        PendingUpdateDescriptor {
            parent_pid: 4242,
            expected_new_version: "1.2.3".to_owned(),
            install_root: root.clone(),
            current_bundle: root.join("SnappyRef.app"),
            staged_bundle: root.join("updates/staging-1.2.3/SnappyRef/SnappyRef.app"),
            staging_dir: root.join("updates/staging-1.2.3"),
            launch_args: vec!["--foo".to_owned(), "bar".to_owned()],
        }
    }

    #[test]
    fn write_creates_parent_directories_and_read_round_trips() {
        let root = crate::test_support::TempRoot::new("descriptor");
        let path = root.join("updates/nested/pending.json");
        let original = descriptor();
        write(&path, &original).unwrap();
        assert_eq!(read(&path).unwrap(), original);
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
}
