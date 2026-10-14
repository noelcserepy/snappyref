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

#[test]
fn helper_decodes_extensionless_webp_download_after_redirect() {
    use std::io::{Read, Write};
    let root = TempRoot::new("snappyref-web-download-helper");
    let source = root.join("browser-image");
    let rgba_output = root.join("output.rgba");
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 1, image::Rgba(PIXEL))
        .write_to(&mut bytes, image::ImageFormat::WebP)
        .unwrap();
    let bytes = bytes.into_inner();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let url = format!("http://{}/image?width=800", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for redirect in [true, false] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            if redirect {
                stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /content?format=webp\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
                stream.write_all(&bytes).unwrap();
            }
        }
    });
    let bytes = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
        .get(url)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .bytes()
        .unwrap();
    std::fs::write(&source, bytes).unwrap();
    server.join().unwrap();
    let output = run_helper(&source, &rgba_output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prepared: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(prepared["source_width"], 2);
    assert_eq!(prepared["source_height"], 1);
    assert_eq!(prepared["mime_type"], "image/webp");
    assert_eq!(
        std::fs::read(&rgba_output).unwrap(),
        [30, 100, 220, 255, 30, 100, 220, 255]
    );
}
