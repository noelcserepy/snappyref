//! Integration tests for the `--imageprep` helper subprocess.
//!
//! Unit tests decode in-process because `cfg(test)` bypasses the helper, so
//! these run the real binary to cover its arguments, stdout JSON and files.

use std::path::Path;
use std::process::{Command, Output};

#[path = "../src/test_support.rs"]
mod test_support;
use test_support::TempRoot;

const PIXEL: [u8; 4] = [30, 100, 220, 255];

fn run_helper(source: &Path, rgba_output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_snappyref"))
        .arg("--imageprep")
        .arg(format!("--source={}", source.display()))
        .arg(format!("--rgba-output={}", rgba_output.display()))
        .arg("--canvas-max-side=16")
        .output()
        .unwrap()
}

#[test]
fn helper_downscales_and_writes_exact_pixels() {
    let root = TempRoot::new("snappyref-imageprep-it");
    let source = root.join("source.png");
    let rgba_output = root.join("output.rgba");
    image::RgbaImage::from_pixel(64, 32, image::Rgba(PIXEL))
        .save(&source)
        .unwrap();

    let output = run_helper(&source, &rgba_output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prepared: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(prepared["source_width"], 64);
    assert_eq!(prepared["source_height"], 32);
    assert_eq!(prepared["width"], 16);
    assert_eq!(prepared["height"], 8);
    assert_eq!(std::fs::read(&rgba_output).unwrap(), PIXEL.repeat(16 * 8));
    assert!(Path::new(prepared["cache_path"].as_str().unwrap()).is_file());
}

#[test]
fn helper_rejects_corrupt_image() {
    let root = TempRoot::new("snappyref-imageprep-it");
    let source = root.join("source.png");
    let rgba_output = root.join("output.rgba");
    std::fs::write(&source, b"not an image").unwrap();

    let output = run_helper(&source, &rgba_output);
    assert!(!output.status.success());
    assert!(!rgba_output.exists());
}

#[test]
fn helper_reports_malformed_xpm_without_panicking() {
    let root = TempRoot::new("snappyref-imageprep-xpm");
    let source = root.join("malformed.xpm");
    let rgba_output = root.join("output.rgba");
    std::fs::write(&source, "\"1 1 1 1\" \"é c #ff0000\" \"a\"").unwrap();
    let output = run_helper(&source, &rgba_output);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("XPM strings must contain only ASCII characters")
    );
}
