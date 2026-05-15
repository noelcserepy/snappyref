//! Windows single-exe update apply path.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::updater::descriptor::{self, PendingUpdateDescriptor};
use crate::updater::install_paths;

const RUNNER_ARG: &str = "--windows-self-replace-runner";
const RUNNER_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const RUNNER_RETRY_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(crate) enum ApplyError {
    Io {
        step: &'static str,
        error: io::Error,
    },
    Descriptor(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::Io { step, error } => write!(f, "windows self-replace {step}: {error}"),
            ApplyError::Descriptor(error) => write!(f, "windows self-replace descriptor: {error}"),
        }
    }
}

impl std::error::Error for ApplyError {}

pub(crate) fn apply_and_relaunch(
    descriptor: &PendingUpdateDescriptor,
    descriptor_path: &Path,
) -> Result<(), ApplyError> {
    descriptor::write(descriptor_path, descriptor)
        .map_err(|error| ApplyError::Descriptor(error.to_string()))?;
    spawn_runner(&descriptor.install_root, descriptor_path)
}

pub fn run_runner_from_args() -> i32 {
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == RUNNER_ARG => {
            match run_runner(&PathBuf::from(path)) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("snappyref windows self-replace runner: {error}");
                    1
                }
            }
        }
        _ => {
            eprintln!("snappyref windows self-replace runner: invalid arguments");
            2
        }
    }
}

pub(crate) fn launch_args_contain_runner_flag(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == RUNNER_ARG)
}

fn spawn_runner(install_root: &Path, descriptor_path: &Path) -> Result<(), ApplyError> {
    let current_exe = std::env::current_exe().map_err(|error| ApplyError::Io {
        step: "resolve current exe",
        error,
    })?;
    let runner_path = runner_path(install_root);
    if let Some(parent) = runner_path.parent() {
        fs::create_dir_all(parent).map_err(|error| ApplyError::Io {
            step: "create runner directory",
            error,
        })?;
    }
    fs::copy(&current_exe, &runner_path).map_err(|error| ApplyError::Io {
        step: "copy runner exe",
        error,
    })?;
    sync_file(&runner_path).map_err(|error| ApplyError::Io {
        step: "sync runner exe",
        error,
    })?;
    Command::new(&runner_path)
        .arg(RUNNER_ARG)
        .arg(descriptor_path)
        .spawn()
        .map_err(|error| ApplyError::Io {
            step: "spawn runner exe",
            error,
        })?;
    Ok(())
}

fn run_runner(descriptor_path: &Path) -> Result<(), ApplyError> {
    let descriptor = descriptor::read(descriptor_path)
        .map_err(|error| ApplyError::Descriptor(error.to_string()))?;
    wait_for_swap(&descriptor.current_bundle, &descriptor.staged_bundle)?;
    Command::new(&descriptor.current_bundle)
        .args(&descriptor.launch_args)
        .spawn()
        .map_err(|error| ApplyError::Io {
            step: "spawn relaunched app",
            error,
        })?;
    let _ = fs::remove_dir_all(&descriptor.staging_dir);
    let _ = fs::remove_file(descriptor_path);
    Ok(())
}

fn wait_for_swap(current_exe: &Path, staged_exe: &Path) -> Result<(), ApplyError> {
    let deadline = Instant::now() + RUNNER_WAIT_TIMEOUT;
    loop {
        match perform_file_swap(current_exe, staged_exe) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error);
                }
            }
        }
        std::thread::sleep(RUNNER_RETRY_INTERVAL);
    }
}

pub(crate) fn perform_file_swap(current_exe: &Path, staged_exe: &Path) -> Result<(), ApplyError> {
    let backup = backup_path_for(current_exe);
    let new_slot = new_path_for(current_exe);

    remove_if_exists(&new_slot).map_err(|error| ApplyError::Io {
        step: "remove stale new slot",
        error,
    })?;
    fs::copy(staged_exe, &new_slot).map_err(|error| ApplyError::Io {
        step: "copy staged exe to new slot",
        error,
    })?;
    sync_file(&new_slot).map_err(|error| ApplyError::Io {
        step: "sync new slot",
        error,
    })?;

    remove_if_exists(&backup).map_err(|error| ApplyError::Io {
        step: "remove prior backup",
        error,
    })?;
    fs::rename(current_exe, &backup).map_err(|error| ApplyError::Io {
        step: "rename current exe to backup",
        error,
    })?;

    if let Err(error) = fs::rename(&new_slot, current_exe) {
        let _ = fs::rename(&backup, current_exe);
        return Err(ApplyError::Io {
            step: "rename new slot to current exe",
            error,
        });
    }

    Ok(())
}

fn runner_path(install_root: &Path) -> PathBuf {
    install_paths::updates_dir(install_root).join(format!(
        "snappyref-self-replace-runner-{}.exe",
        std::process::id()
    ))
}

fn backup_path_for(current_exe: &Path) -> PathBuf {
    sibling_with_suffix(current_exe, ".old")
}

fn new_path_for(current_exe: &Path) -> PathBuf {
    sibling_with_suffix(current_exe, ".new")
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.parent()
        .map(|parent| parent.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "snappyref-windows-self-replace-{prefix}-{}-{}-{nanos}",
            std::process::id(),
            counter,
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_swap_replaces_current_exe_and_keeps_old_backup() {
        let dir = unique_temp_dir("swap-ok");
        let current = dir.join("snappyref.exe");
        let staged = dir.join("updates/staging-1.0.0/SnappyRef/snappyref.exe");
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(&current, b"old-exe").unwrap();
        fs::write(&staged, b"new-exe").unwrap();

        perform_file_swap(&current, &staged).unwrap();

        assert_eq!(fs::read(&current).unwrap(), b"new-exe");
        assert_eq!(fs::read(dir.join("snappyref.exe.old")).unwrap(), b"old-exe");
        assert!(staged.exists(), "staged exe remains as recovery source");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_swap_replaces_prior_backup() {
        let dir = unique_temp_dir("replace-backup");
        let current = dir.join("snappyref.exe");
        let staged = dir.join("updates/staging-1.0.0/SnappyRef/snappyref.exe");
        let backup = dir.join("snappyref.exe.old");
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::write(&current, b"old-exe").unwrap();
        fs::write(&staged, b"new-exe").unwrap();
        fs::write(&backup, b"older-backup").unwrap();

        perform_file_swap(&current, &staged).unwrap();

        assert_eq!(fs::read(&current).unwrap(), b"new-exe");
        assert_eq!(fs::read(&backup).unwrap(), b"old-exe");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn runner_path_lives_inside_updates_dir() {
        let install_root = Path::new("C:/SnappyRef");
        let path = runner_path(install_root);

        assert!(path.starts_with(install_paths::updates_dir(install_root)));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("exe"));
    }
}
