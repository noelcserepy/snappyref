//! Real Windows runner with a process denying deletion of the live executable.
#![cfg(windows)]

use std::fs;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use snappyref::updater::descriptor::{self, PendingUpdateDescriptor};
#[path = "../src/test_support.rs"]
mod test_support;
use test_support::TempRoot;

// A read-sharing handle denies the rename/delete needed for replacement.
const FILE_SHARE_READ: u32 = 0x1;

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "{} was not written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn fixture_command() -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "runner_process_fixture",
            "--ignored",
            "--nocapture",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
#[ignore = "subprocess fixture for the runner test"]
fn runner_process_fixture() {
    let root = std::path::PathBuf::from(std::env::var_os("RUNNER_TEST_ROOT").unwrap());
    match std::env::var("RUNNER_TEST_ROLE").unwrap().as_str() {
        "holder" => {
            let _held = fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(root.join("snappyref.exe"))
                .unwrap();
            fs::write(root.join("held"), b"locked").unwrap();
            wait_for_file(&root.join("release"));
        }
        "relaunched" => {
            let args: Vec<_> = std::env::args().skip(1).collect();
            fs::write(
                root.join("relaunched.json"),
                serde_json::to_vec(&args).unwrap(),
            )
            .unwrap();
        }
        role => panic!("unknown fixture role {role}"),
    }
}

#[test]
fn runner_waits_for_executable_release_swaps_relaunches_and_cleans_staging() {
    let root = TempRoot::new("windows-runner");
    let current = root.join("snappyref.exe");
    let staging = root.join("updates/staging-1.2.3");
    let staged = staging.join("SnappyRef/snappyref.exe");
    fs::create_dir_all(staged.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_snappyref"), &current).unwrap();
    let old_bytes = fs::read(&current).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &staged).unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&staged)
        .unwrap()
        .write_all(b"new-version-marker")
        .unwrap();
    let new_bytes = fs::read(&staged).unwrap();
    let mut holder = Process(
        fixture_command()
            .env("RUNNER_TEST_ROLE", "holder")
            .env("RUNNER_TEST_ROOT", &*root)
            .spawn()
            .unwrap(),
    );
    wait_for_file(&root.join("held"));
    let args = vec![
        "--exact".into(),
        "runner_process_fixture".into(),
        "--ignored".into(),
        "--nocapture".into(),
    ];
    let path = descriptor::default_path(&root);
    descriptor::write(
        &path,
        &PendingUpdateDescriptor {
            parent_pid: holder.0.id(),
            expected_new_version: "1.2.3".into(),
            install_root: root.to_path_buf(),
            current_bundle: current.clone(),
            staged_bundle: staged.clone(),
            staging_dir: staging.clone(),
            launch_args: args,
        },
    )
    .unwrap();
    let runner_path = root.join("updates/runner.exe");
    fs::copy(env!("CARGO_BIN_EXE_snappyref"), &runner_path).unwrap();
    let mut runner = Process(
        Command::new(&runner_path)
            .arg("--windows-self-replace-runner")
            .arg(&path)
            .env("RUNNER_TEST_ROLE", "relaunched")
            .env("RUNNER_TEST_ROOT", &*root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(runner.0.try_wait().unwrap().is_none());
    assert_eq!(fs::read(&current).unwrap(), old_bytes);
    assert_eq!(fs::read(&staged).unwrap(), new_bytes);
    fs::write(root.join("release"), b"exit").unwrap();
    assert!(holder.0.wait().unwrap().success());
    wait_for_file(&root.join("relaunched.json"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = runner.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "runner did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read(&current).unwrap(), new_bytes);
    assert_eq!(fs::read(root.join("snappyref.exe.old")).unwrap(), old_bytes);
    let launched: Vec<String> =
        serde_json::from_slice(&fs::read(root.join("relaunched.json")).unwrap()).unwrap();
    assert_eq!(
        launched,
        [
            "--exact",
            "runner_process_fixture",
            "--ignored",
            "--nocapture"
        ]
    );
    assert!(!staging.exists());
    assert!(!path.exists());
    assert!(!root.join("snappyref.exe.new").exists());
}
