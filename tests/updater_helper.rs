//! macOS integration tests for the `snappyref-updater` helper binary.
//!
//! The helper's job is to wait for the running app to exit, swap the
//! current bundle with a staged one, and relaunch. These tests exercise
//! the full end-to-end flow against a tempdir install root, using a
//! sleep-based fake parent process and a relaunch-override env so the
//! test can assert the relaunch happened without spinning up a GUI.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use snappyref::updater::descriptor::{self, PendingUpdateDescriptor};

#[path = "../src/test_support.rs"]
mod test_support;
use test_support::TempRoot;

/// Build a tempdir layout that mirrors a real install root, populated with
/// a stale "current" bundle and a freshly-staged bundle ready to swap in.
struct Fixture {
    _root: TempRoot,
    install_root: PathBuf,
    staging_dir: PathBuf,
    current_bundle: PathBuf,
    staged_bundle: PathBuf,
    relaunch_marker: PathBuf,
    library_dir: PathBuf,
    models_dir: PathBuf,
}

impl Fixture {
    fn new(version: &str) -> Self {
        let root = TempRoot::new("snappyref-updater-it");
        let install_root = root.to_path_buf();

        // Untouchables: library/ and models/ must survive the swap.
        let library_dir = install_root.join("library");
        let models_dir = install_root.join("models");
        fs::create_dir_all(&library_dir).unwrap();
        fs::create_dir_all(&models_dir).unwrap();
        fs::write(library_dir.join("workspace.db"), b"library-data").unwrap();
        fs::write(models_dir.join("model.bin"), b"model-data").unwrap();

        let staging_dir = install_root
            .join("updates")
            .join(format!("staging-{version}"));
        let staged_top = staging_dir.join("SnappyRef");
        fs::create_dir_all(&staged_top).unwrap();

        let current_bundle = install_root.join("SnappyRef.app");
        let current_macos = current_bundle.join("Contents/MacOS");
        fs::create_dir_all(&current_macos).unwrap();
        fs::write(current_macos.join("snappyref"), b"old-binary").unwrap();
        fs::write(current_bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();

        let staged_bundle = staged_top.join("SnappyRef.app");
        let staged_macos = staged_bundle.join("Contents/MacOS");
        fs::create_dir_all(&staged_macos).unwrap();
        fs::write(staged_macos.join("snappyref"), b"new-binary").unwrap();
        fs::write(staged_macos.join("snappyref-updater"), b"new-helper").unwrap();
        fs::write(staged_bundle.join("Contents/Info.plist"), b"<plist/>").unwrap();

        let relaunch_marker = install_root.join("relaunch-marker.txt");

        Self {
            _root: root,
            install_root,
            staging_dir,
            current_bundle,
            staged_bundle,
            relaunch_marker,
            library_dir,
            models_dir,
        }
    }

    fn backup_path(&self) -> PathBuf {
        let name = self
            .current_bundle
            .file_name()
            .map(|n| {
                let mut s = n.to_os_string();
                s.push(".previous");
                s
            })
            .unwrap();
        self.current_bundle.parent().unwrap().join(name)
    }
}

/// Spawn a long-lived child process to act as the "running app" the helper
/// must wait on.
fn spawn_fake_parent() -> std::process::Child {
    Command::new("/bin/sleep")
        .arg("30")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake parent")
}

/// Resolve a "touch a file" program that takes args. On Unix `/bin/sh -c
/// 'echo "$@" > marker; ...'` is awkward through Command; we instead use a
/// minimal cross-platform shape: the helper passes `<bundle> <args...>` so
/// the override program writes its argv to the marker.
///
/// For simplicity in tests, we use a small shell script we write into the
/// install root and point `SNAPPYREF_UPDATER_RELAUNCH_PROGRAM` at it.
fn write_relaunch_override_script(install_root: &Path, marker: &Path) -> PathBuf {
    let script = install_root.join("relaunch.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"RELAUNCHED $@\" > \"{}\"\n",
            marker.display()
        ),
    )
    .unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    script
}

fn wait_for_marker(marker: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if marker.is_file() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    marker.is_file()
}

/// End-to-end: the helper waits for the parent to exit, performs the
/// platform swap, fires the relaunch override, and removes the staging
/// directory. The library/ and models/ trees must be untouched.
#[test]
fn helper_swaps_bundle_and_relaunches_after_parent_exits() {
    let fixture = Fixture::new("1.2.3");
    let mut parent = spawn_fake_parent();
    let parent_pid = parent.id();

    let descriptor_value = PendingUpdateDescriptor {
        parent_pid,
        expected_new_version: "1.2.3".to_owned(),
        install_root: fixture.install_root.clone(),
        current_bundle: fixture.current_bundle.clone(),
        staged_bundle: fixture.staged_bundle.clone(),
        staging_dir: fixture.staging_dir.clone(),
        launch_args: vec!["--from-test".to_owned()],
    };
    let descriptor_path = descriptor::default_path(&fixture.install_root);
    descriptor::write(&descriptor_path, &descriptor_value).unwrap();

    let relaunch_script =
        write_relaunch_override_script(&fixture.install_root, &fixture.relaunch_marker);

    let helper_exe = env!("CARGO_BIN_EXE_snappyref-updater");
    let mut helper = Command::new(helper_exe)
        .arg(&descriptor_path)
        .env("SNAPPYREF_UPDATER_PARENT_WAIT_TIMEOUT_MS", "10000")
        .env("SNAPPYREF_UPDATER_RELAUNCH_PROGRAM", &relaunch_script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn helper");

    // Give the helper a moment to start polling, then kill the fake parent
    // so the wait-loop sees the PID go away.
    std::thread::sleep(Duration::from_millis(200));
    let _ = parent.kill();
    let _ = parent.wait();

    let status = helper.wait().expect("helper exit");
    if !status.success() {
        use std::io::Read as _;
        let mut stderr = String::new();
        if let Some(mut e) = helper.stderr.take() {
            let _ = e.read_to_string(&mut stderr);
        }
        panic!("helper exited with {status}: stderr={stderr}");
    }

    // Swap: the staged bundle's content must now live at current_bundle,
    // and the old current bundle must have been renamed to its backup
    // suffix.
    assert!(
        fixture.current_bundle.exists(),
        "current bundle must exist at swap target"
    );
    let backup = fixture.backup_path();
    assert!(backup.exists(), "backup must exist at {}", backup.display());

    // Confirm the swapped content is the new bundle's payload.
    let probe = fs::read(fixture.current_bundle.join("Contents/MacOS/snappyref")).unwrap();
    assert_eq!(probe, b"new-binary");

    // The old payload survives in the backup.
    let backup_probe = fs::read(backup.join("Contents/MacOS/snappyref")).unwrap();
    assert_eq!(backup_probe, b"old-binary");

    // Relaunch fired with the swapped current_bundle path and launch args.
    assert!(
        wait_for_marker(&fixture.relaunch_marker, Duration::from_secs(5)),
        "relaunch override did not run within timeout"
    );
    let marker = fs::read_to_string(&fixture.relaunch_marker).unwrap();
    assert!(
        marker.contains(&*fixture.current_bundle.to_string_lossy()),
        "relaunch marker missing current bundle path: {marker}",
    );
    assert!(
        marker.contains("--from-test"),
        "relaunch marker missing forwarded launch args: {marker}",
    );

    // Staging directory must be removed after a successful relaunch.
    assert!(
        !fixture.staging_dir.exists(),
        "staging dir must be removed; still present at {}",
        fixture.staging_dir.display()
    );

    // library/ and models/ are untouched.
    assert_eq!(
        fs::read(fixture.library_dir.join("workspace.db")).unwrap(),
        b"library-data"
    );
    assert_eq!(
        fs::read(fixture.models_dir.join("model.bin")).unwrap(),
        b"model-data"
    );
}

/// A pre-existing backup must be overwritten by the swap, not nested
/// inside the new one. Only one prior version is retained.
#[test]
fn helper_overwrites_existing_backup() {
    let fixture = Fixture::new("1.2.4");

    // Plant a stale backup from a previous update.
    let backup = fixture.backup_path();
    fs::create_dir_all(backup.join("Contents/MacOS")).unwrap();
    fs::write(backup.join("Contents/MacOS/snappyref"), b"older-binary").unwrap();

    let mut parent = spawn_fake_parent();
    let parent_pid = parent.id();

    let descriptor_value = PendingUpdateDescriptor {
        parent_pid,
        expected_new_version: "1.2.4".to_owned(),
        install_root: fixture.install_root.clone(),
        current_bundle: fixture.current_bundle.clone(),
        staged_bundle: fixture.staged_bundle.clone(),
        staging_dir: fixture.staging_dir.clone(),
        launch_args: vec![],
    };
    let descriptor_path = descriptor::default_path(&fixture.install_root);
    descriptor::write(&descriptor_path, &descriptor_value).unwrap();

    let relaunch_script =
        write_relaunch_override_script(&fixture.install_root, &fixture.relaunch_marker);

    let helper_exe = env!("CARGO_BIN_EXE_snappyref-updater");
    let mut helper = Command::new(helper_exe)
        .arg(&descriptor_path)
        .env("SNAPPYREF_UPDATER_PARENT_WAIT_TIMEOUT_MS", "10000")
        .env("SNAPPYREF_UPDATER_RELAUNCH_PROGRAM", &relaunch_script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn helper");

    std::thread::sleep(Duration::from_millis(200));
    let _ = parent.kill();
    let _ = parent.wait();
    let status = helper.wait().expect("helper exit");
    assert!(status.success(), "helper exited with {status}");

    // Backup now contains the just-replaced "old-binary", not the
    // pre-existing "older-binary".
    let backup_probe = fs::read(backup.join("Contents/MacOS/snappyref")).unwrap();
    assert_eq!(backup_probe, b"old-binary");
}

/// Malformed descriptors must be rejected before any filesystem damage.
#[test]
fn helper_rejects_malformed_descriptor() {
    let fixture = Fixture::new("1.0.0");
    let descriptor_path = descriptor::default_path(&fixture.install_root);
    fs::create_dir_all(descriptor_path.parent().unwrap()).unwrap();
    fs::write(&descriptor_path, b"not valid json").unwrap();

    let helper_exe = env!("CARGO_BIN_EXE_snappyref-updater");
    let output = Command::new(helper_exe)
        .arg(&descriptor_path)
        .env("SNAPPYREF_UPDATER_PARENT_WAIT_TIMEOUT_MS", "500")
        .output()
        .expect("run helper");

    assert!(
        !output.status.success(),
        "helper must reject malformed descriptor",
    );
    // Current bundle must be untouched; no swap occurred.
    assert!(fixture.current_bundle.exists());
    let probe = fs::read(fixture.current_bundle.join("Contents/MacOS/snappyref")).unwrap();
    assert_eq!(probe, b"old-binary");
}

/// Moving the current bundle also removes this deliberately nested source path,
/// causing the actual promotion rename to fail after backup succeeds.
#[test]
fn helper_restores_current_bundle_when_promotion_fails() {
    let fixture = Fixture::new("9.9.9");
    let staged_bundle = fixture.current_bundle.join("nested-staging.app");
    fs::create_dir_all(&staged_bundle).unwrap();
    let value = PendingUpdateDescriptor {
        parent_pid: 0,
        expected_new_version: "9.9.9".to_owned(),
        install_root: fixture.install_root.clone(),
        current_bundle: fixture.current_bundle.clone(),
        staged_bundle,
        staging_dir: fixture.staging_dir.clone(),
        launch_args: vec![],
    };
    let path = descriptor::default_path(&fixture.install_root);
    descriptor::write(&path, &value).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_snappyref-updater"))
        .arg(path)
        .env("SNAPPYREF_UPDATER_PARENT_WAIT_TIMEOUT_MS", "0")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to move staged bundle"));
    assert_eq!(
        fs::read(fixture.current_bundle.join("Contents/MacOS/snappyref")).unwrap(),
        b"old-binary"
    );
    assert!(!fixture.backup_path().exists());
    assert_eq!(
        fs::read(fixture.library_dir.join("workspace.db")).unwrap(),
        b"library-data"
    );
    assert_eq!(
        fs::read(fixture.models_dir.join("model.bin")).unwrap(),
        b"model-data"
    );
}
