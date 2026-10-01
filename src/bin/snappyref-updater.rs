//! The `snappyref-updater` helper binary.
//!
//! Ships inside the macOS app bundle next to the main `snappyref` executable
//! (`SnappyRef.app/Contents/MacOS/snappyref-updater`). The running app
//! launches this helper just before exiting and hands it the path to a
//! [`PendingUpdateDescriptor`] JSON file on disk; the helper then:
//!
//! 1. reads + validates the descriptor;
//! 2. waits for the parent app's PID to exit (polling, with a timeout);
//! 3. strips `com.apple.quarantine` from the staged bundle on macOS;
//! 4. renames the current bundle to a `.previous` backup,
//!    overwriting any prior backup;
//! 5. moves the staged bundle into the current bundle's location, moving the
//!    backup back if that fails;
//! 6. relaunches the new app with the original launch args;
//! 7. removes the staging directory.
//!
//! Failures are written to stderr and exit with a non-zero code, never to a
//! GUI dialog — the helper is a console process. Exit code 2 means the
//! restore also failed and the prior app is only in the `.previous` backup.
//! The auto-update flow stays inside the user's portable install folder and
//! never touches `library/` or `models/`.

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    macos::main()
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(all(test, target_os = "macos"))]
#[path = "../test_support.rs"]
mod test_support;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode};
    use std::time::{Duration, Instant};

    use snappyref::updater::descriptor::{self, PendingUpdateDescriptor};

    /// Maximum time the helper waits for the parent app to exit before
    /// proceeding anyway. The fallback is a defensive cap; in practice the app
    /// has already called `eframe::ViewportCommand::Close` before launching us.
    const DEFAULT_PARENT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
    /// Polling cadence while waiting for the parent PID. A short interval keeps
    /// total wait latency low when the app exits cleanly within milliseconds.
    const PARENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

    /// Env override for the parent-wait timeout, in milliseconds. Used by the
    /// integration test fixture to keep the wait snappy.
    const PARENT_WAIT_TIMEOUT_ENV: &str = "SNAPPYREF_UPDATER_PARENT_WAIT_TIMEOUT_MS";
    /// Env override for the relaunch command. When set, the helper invokes
    /// `<program> <new_bundle> <launch_args...>` instead of `open`.
    /// Used by the integration test to assert the relaunch contract without
    /// actually starting a GUI app.
    const RELAUNCH_OVERRIDE_ENV: &str = "SNAPPYREF_UPDATER_RELAUNCH_PROGRAM";

    /// Exit code when the prior app survives only in its backup and the
    /// installed path is empty. Any other failure exits with 1.
    const RECOVERY_REQUIRED_EXIT_CODE: u8 = 2;

    #[derive(Debug)]
    enum Failure {
        /// The installed path holds what it held before the helper ran.
        Failed(String),
        /// Promotion and restore both failed. The prior app is only at
        /// `backup`, which the helper leaves in place.
        RecoveryRequired { backup: PathBuf, reason: String },
    }

    impl From<String> for Failure {
        fn from(reason: String) -> Self {
            Self::Failed(reason)
        }
    }

    pub(super) fn main() -> ExitCode {
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(Failure::Failed(reason)) => {
                eprintln!("snappyref-updater: {reason}");
                ExitCode::FAILURE
            }
            Err(Failure::RecoveryRequired { backup, reason }) => {
                eprintln!(
                    "snappyref-updater: recovery required, prior app retained at `{}`: {reason}",
                    backup.display()
                );
                ExitCode::from(RECOVERY_REQUIRED_EXIT_CODE)
            }
        }
    }

    fn run() -> Result<(), Failure> {
        let descriptor_path = parse_args()?;
        let descriptor = descriptor::read(&descriptor_path).map_err(|error| error.to_string())?;
        let timeout = resolve_parent_wait_timeout();
        wait_for_parent_exit(descriptor.parent_pid, timeout);
        apply_swap(&descriptor, |from, to| std::fs::rename(from, to))?;
        relaunch(&descriptor)?;
        cleanup_staging(&descriptor.staging_dir);
        Ok(())
    }

    fn parse_args() -> Result<PathBuf, String> {
        let mut args = std::env::args_os();
        let _argv0 = args.next();
        let path = args
            .next()
            .ok_or_else(|| "missing descriptor path argument".to_owned())?;
        if args.next().is_some() {
            return Err("too many arguments; expected one descriptor path".to_owned());
        }
        Ok(PathBuf::from(path))
    }

    fn resolve_parent_wait_timeout() -> Duration {
        match std::env::var(PARENT_WAIT_TIMEOUT_ENV) {
            Ok(value) => value
                .parse::<u64>()
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_PARENT_WAIT_TIMEOUT),
            Err(_) => DEFAULT_PARENT_WAIT_TIMEOUT,
        }
    }

    /// Poll `parent_pid` until it no longer exists or `timeout` elapses.
    /// Returns whether the parent actually exited within the timeout; the
    /// helper still proceeds with the swap either way. A wedged GUI process
    /// must not block the relaunch forever.
    pub fn wait_for_parent_exit(parent_pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if !is_process_running(parent_pid) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(PARENT_POLL_INTERVAL);
        }
    }

    fn is_process_running(pid: u32) -> bool {
        // SAFETY: kill(pid, 0) is a probe-only syscall with no signal sent;
        // it returns 0 when the process exists and we have permission to
        // signal it, or -1 with errno set otherwise. EPERM still means the
        // process is alive, so we treat a non-ESRCH error as "running".
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            return true;
        }
        // errno == ESRCH ⇒ no such process. Anything else (EPERM, EINVAL)
        // means the process is alive, just not signalable from us.
        let errno = std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::ESRCH);
        errno != libc::ESRCH
    }

    /// The current bundle is renamed to its `.previous` backup, and the
    /// staged bundle is moved into the slot left behind. If that move fails,
    /// the backup goes back to the installed path. `rename` is the filesystem
    /// move, injectable so tests can fail promotion and restore.
    fn apply_swap(
        descriptor: &PendingUpdateDescriptor,
        rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<(), Failure> {
        if !descriptor.staged_bundle.exists() {
            return Err(Failure::Failed(format!(
                "staged bundle missing at `{}`",
                descriptor.staged_bundle.display()
            )));
        }

        strip_quarantine_xattr(&descriptor.staged_bundle);

        let backup_path = backup_path_for(&descriptor.current_bundle);
        if let Ok(metadata) = std::fs::symlink_metadata(&backup_path) {
            let result = if metadata.is_dir() {
                std::fs::remove_dir_all(&backup_path)
            } else {
                std::fs::remove_file(&backup_path)
            };
            result.map_err(|error| {
                format!(
                    "failed to remove prior backup `{}`: {error}",
                    backup_path.display()
                )
            })?;
        }

        let backed_up = descriptor.current_bundle.exists();
        if backed_up {
            rename(&descriptor.current_bundle, &backup_path).map_err(|error| {
                format!(
                    "failed to back up `{}` to `{}`: {error}",
                    descriptor.current_bundle.display(),
                    backup_path.display()
                )
            })?;
        }

        let Err(error) = rename(&descriptor.staged_bundle, &descriptor.current_bundle) else {
            return Ok(());
        };
        let reason = format!(
            "failed to move staged bundle into place at `{}`: {error}",
            descriptor.current_bundle.display()
        );
        if !backed_up {
            return Err(Failure::Failed(reason));
        }
        match rename(&backup_path, &descriptor.current_bundle) {
            Ok(()) => Err(Failure::Failed(format!("{reason}; prior app restored"))),
            Err(restore_error) => Err(Failure::RecoveryRequired {
                backup: backup_path,
                reason: format!("{reason}; restoring prior app failed: {restore_error}"),
            }),
        }
    }

    /// `<install_root>/SnappyRef.app` → `<install_root>/SnappyRef.app.previous`.
    fn backup_path_for(current: &Path) -> PathBuf {
        let mut name: OsString = current
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".previous");
        current
            .parent()
            .map(|parent| parent.join(&name))
            .unwrap_or_else(|| PathBuf::from(name))
    }

    fn strip_quarantine_xattr(bundle: &Path) {
        // Best-effort: xattr(1) ships with macOS. A failure is non-fatal —
        // in-process reqwest downloads don't set the quarantine bit in the
        // first place, so this is defense-in-depth.
        let _ = Command::new("/usr/bin/xattr")
            .arg("-dr")
            .arg("com.apple.quarantine")
            .arg(bundle)
            .status();
    }

    /// Launch the freshly-swapped app. Test fixtures inject a fake program via
    /// [`RELAUNCH_OVERRIDE_ENV`] so the integration test can assert that the
    /// relaunch fired without actually opening a GUI.
    fn relaunch(descriptor: &PendingUpdateDescriptor) -> Result<(), String> {
        if let Ok(program) = std::env::var(RELAUNCH_OVERRIDE_ENV) {
            return spawn_relaunch_override(&program, descriptor);
        }

        let (program, args) =
            macos_relaunch_argv(&descriptor.current_bundle, &descriptor.launch_args);
        let mut command = Command::new(program);
        command.args(&args);
        command
            .spawn()
            .map_err(|error| format!("failed to relaunch app: {error}"))?;
        Ok(())
    }

    /// Build the `open(1)` argv used to relaunch the app on macOS. Extracted from
    /// [`relaunch`] for unit testing — `open -a <bundle> [--args <forwarded...>]`
    /// forwards extra args to the app's launched process and detaches the helper
    /// from its lifetime.
    pub fn macos_relaunch_argv(
        bundle: &Path,
        launch_args: &[String],
    ) -> (&'static str, Vec<OsString>) {
        let mut args: Vec<OsString> = Vec::with_capacity(3 + launch_args.len());
        args.push(OsString::from("-a"));
        args.push(bundle.as_os_str().to_os_string());
        if !launch_args.is_empty() {
            args.push(OsString::from("--args"));
            args.extend(launch_args.iter().map(OsString::from));
        }
        ("/usr/bin/open", args)
    }

    fn spawn_relaunch_override(
        program: &str,
        descriptor: &PendingUpdateDescriptor,
    ) -> Result<(), String> {
        let mut command = Command::new(program);
        command.arg(&descriptor.current_bundle);
        command.args(&descriptor.launch_args);
        command
            .spawn()
            .map_err(|error| format!("failed to relaunch via override: {error}"))?;
        Ok(())
    }

    fn cleanup_staging(staging_dir: &Path) {
        if staging_dir.exists() {
            let _ = std::fs::remove_dir_all(staging_dir);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn macos_relaunch_argv_without_launch_args() {
            let bundle = PathBuf::from("/Applications/SnappyRef.app");
            let (program, args) = macos_relaunch_argv(&bundle, &[]);
            assert_eq!(program, "/usr/bin/open");
            assert_eq!(args, vec![OsString::from("-a"), OsString::from(&bundle)]);
        }

        #[test]
        fn macos_relaunch_argv_with_launch_args_inserts_args_separator() {
            let bundle = PathBuf::from("/Applications/SnappyRef.app");
            let launch_args = vec!["--profile".to_owned(), "default".to_owned()];
            let (program, args) = macos_relaunch_argv(&bundle, &launch_args);
            assert_eq!(program, "/usr/bin/open");
            assert_eq!(
                args,
                vec![
                    OsString::from("-a"),
                    OsString::from(&bundle),
                    OsString::from("--args"),
                    OsString::from("--profile"),
                    OsString::from("default"),
                ]
            );
        }

        #[test]
        fn backup_path_for_appends_previous_suffix() {
            let current = PathBuf::from("/Applications/SnappyRef.app");
            let backup = backup_path_for(&current);
            let suffix = ".previous";
            let expected_name = format!(
                "{}{}",
                current.file_name().unwrap().to_string_lossy(),
                suffix
            );
            assert_eq!(backup.file_name().unwrap().to_string_lossy(), expected_name);
            assert_eq!(backup.parent(), current.parent());
        }

        #[test]
        fn failed_restore_keeps_backup_and_requires_recovery() {
            let root = crate::test_support::TempRoot::new("snappyref-updater-restore");
            let current_bundle = root.join("SnappyRef.app");
            let staged_bundle = root.join("staging/SnappyRef.app");
            std::fs::create_dir_all(&current_bundle).unwrap();
            std::fs::write(current_bundle.join("snappyref"), b"old-binary").unwrap();
            std::fs::create_dir_all(&staged_bundle).unwrap();
            let descriptor = PendingUpdateDescriptor {
                parent_pid: 0,
                expected_new_version: "9.9.9".to_owned(),
                install_root: root.to_path_buf(),
                current_bundle: current_bundle.clone(),
                staged_bundle,
                staging_dir: root.join("staging"),
                launch_args: vec![],
            };

            // Promotion and restore both move into the installed path.
            let result = apply_swap(&descriptor, |from, to| {
                if to == current_bundle {
                    Err(std::io::Error::other("injected"))
                } else {
                    std::fs::rename(from, to)
                }
            });

            let Err(Failure::RecoveryRequired { backup, .. }) = result else {
                panic!("expected recovery-required, got {result:?}");
            };
            assert_eq!(backup, backup_path_for(&current_bundle));
            assert_eq!(
                std::fs::read(backup.join("snappyref")).unwrap(),
                b"old-binary"
            );
            assert!(!current_bundle.exists());
        }
    }
}
