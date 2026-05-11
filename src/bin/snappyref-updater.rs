//! The `snappyref-updater` helper binary.
//!
//! Ships inside the bundle/folder next to the main `snappyref` executable
//! (`SnappyRef.app/Contents/MacOS/snappyref-updater` on macOS,
//! `SnappyRef/snappyref-updater.exe` on Windows). The running app launches
//! this helper just before exiting and hands it the path to a
//! [`PendingUpdateDescriptor`] JSON file on disk; the helper then:
//!
//! 1. reads + validates the descriptor;
//! 2. waits for the parent app's PID to exit (polling, with a timeout);
//! 3. strips `com.apple.quarantine` from the staged bundle on macOS;
//! 4. renames the current bundle to a `.previous` / `.old` backup,
//!    overwriting any prior backup;
//! 5. moves the staged bundle into the current bundle's location;
//! 6. relaunches the new app with the original launch args;
//! 7. removes the staging directory.
//!
//! Failures are written to stderr and exit with a non-zero code, never to a
//! GUI dialog — the helper is a console process. ADR-0015: the auto-update
//! flow stays inside the user's portable install folder and never touches
//! `library/` or `models/`.

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
/// `<program> <new_bundle> <launch_args...>` instead of the platform's
/// native launcher (`open` on macOS, the new exe directly on Windows).
/// Used by the integration test to assert the relaunch contract without
/// actually starting a GUI app.
const RELAUNCH_OVERRIDE_ENV: &str = "SNAPPYREF_UPDATER_RELAUNCH_PROGRAM";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("snappyref-updater: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let descriptor_path = parse_args()?;
    let descriptor = descriptor::read(&descriptor_path).map_err(|error| error.to_string())?;
    let timeout = resolve_parent_wait_timeout();
    wait_for_parent_exit(descriptor.parent_pid, timeout);
    apply_swap(&descriptor)?;
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
/// helper still proceeds with the swap either way per ADR-0015's "timeout
/// fallback" clause — a wedged GUI process must not block the relaunch.
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

#[cfg(unix)]
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

#[cfg(windows)]
fn is_process_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        // Probe with a zero-duration wait: WAIT_TIMEOUT means the process
        // is still running, WAIT_OBJECT_0 means it has exited.
        let wait = WaitForSingleObject(handle, 0);
        if wait == WAIT_TIMEOUT {
            CloseHandle(handle);
            return true;
        }
        let mut code: u32 = 0;
        let got_code = GetExitCodeProcess(handle, &mut code as *mut u32);
        CloseHandle(handle);
        got_code != 0 && code == STILL_ACTIVE as u32
    }
}

/// Perform the platform swap described in ADR-0015. The current bundle is
/// renamed to its `.previous` / `.old` backup (overwriting any prior
/// backup), and the staged bundle is moved into the slot left behind.
pub fn apply_swap(descriptor: &PendingUpdateDescriptor) -> Result<(), String> {
    if !descriptor.staged_bundle.exists() {
        return Err(format!(
            "staged bundle missing at `{}`",
            descriptor.staged_bundle.display()
        ));
    }

    #[cfg(target_os = "macos")]
    strip_quarantine_xattr(&descriptor.staged_bundle);

    let backup_path = backup_path_for(&descriptor.current_bundle);
    if backup_path.exists() {
        std::fs::remove_dir_all(&backup_path)
            .or_else(|_| std::fs::remove_file(&backup_path))
            .map_err(|error| {
                format!(
                    "failed to remove prior backup `{}`: {error}",
                    backup_path.display()
                )
            })?;
    }

    if descriptor.current_bundle.exists() {
        std::fs::rename(&descriptor.current_bundle, &backup_path).map_err(|error| {
            format!(
                "failed to back up `{}` to `{}`: {error}",
                descriptor.current_bundle.display(),
                backup_path.display()
            )
        })?;
    }

    std::fs::rename(&descriptor.staged_bundle, &descriptor.current_bundle).map_err(|error| {
        format!(
            "failed to move staged bundle into place at `{}`: {error}",
            descriptor.current_bundle.display()
        )
    })?;

    Ok(())
}

/// `<install_root>/SnappyRef.app` → `<install_root>/SnappyRef.app.previous`.
/// `<install_root>/snappyref.exe` → `<install_root>/snappyref.exe.old`.
fn backup_path_for(current: &Path) -> PathBuf {
    let mut name: OsString = current
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    if cfg!(target_os = "windows") {
        name.push(".old");
    } else {
        name.push(".previous");
    }
    current
        .parent()
        .map(|parent| parent.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

#[cfg(target_os = "macos")]
fn strip_quarantine_xattr(bundle: &Path) {
    // Best-effort: xattr(1) ships with macOS. A failure is non-fatal —
    // in-process reqwest downloads don't set the quarantine bit in the
    // first place, so this is defense-in-depth per ADR-0015.
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

    #[cfg(target_os = "macos")]
    {
        // `open <app>` with `--args` forwards extra args to the app's
        // launched process. Spawning detached keeps the helper from
        // blocking on the new app's lifetime.
        let mut command = Command::new("/usr/bin/open");
        command.arg("-a").arg(&descriptor.current_bundle);
        if !descriptor.launch_args.is_empty() {
            command.arg("--args");
            command.args(&descriptor.launch_args);
        }
        command
            .spawn()
            .map_err(|error| format!("failed to relaunch app: {error}"))?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Windows + everything else: invoke the new binary directly. The
        // helper does not wait on the child; it exits as soon as the new
        // process is alive.
        let mut command = Command::new(&descriptor.current_bundle);
        command.args(&descriptor.launch_args);
        command
            .spawn()
            .map_err(|error| format!("failed to relaunch app: {error}"))?;
        Ok(())
    }
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
