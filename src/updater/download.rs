//! Download the platform-appropriate zip referenced
//! by the verified manifest, write it into `<install_root>/updates/` with
//! progress callbacks, and verify the sha256 against the manifest.
//!
//! Strict verification is the handoff contract: a mismatched hash never
//! reaches staging and the partial file is cleaned up. The update state
//! machine treats a download failure exactly like a discovery failure
//! (`Failed`).
//!
//! The module is structured around two layers so production code can use
//! `reqwest` while unit tests drive a local HTTP fixture:
//!
//! * [`download_from_reader_cancellable`] streams an arbitrary `Read` into
//!   the target path while invoking the progress callback, then verifies
//!   sha256.
//! * [`download_from_url`] wraps the production path: build a blocking
//!   `reqwest` client, stream the response body, delegate to
//!   `download_from_reader_cancellable`.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

/// HTTP timeout for the download client. Larger than discovery's 20s
/// because zips are tens of megabytes; we still cap it so a wedged TCP
/// connection cannot keep the worker thread alive indefinitely.
pub(crate) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// Streamed read buffer size. 64 KiB is a reasonable trade-off between
/// syscall count and per-callback granularity. With a 10 MiB zip this
/// fires the progress callback ~160 times during the download.
const READ_CHUNK: usize = 64 * 1024;

/// Failure modes for [`download_from_reader_cancellable`]. The caller logs and
/// transitions the update state to `Failed`; the partial file is unlinked before
/// returning.
#[derive(Debug)]
pub(crate) enum DownloadError {
    /// Filesystem error while preparing the destination or writing bytes.
    Io(io::Error),
    /// Network error while reading the response body.
    Network(String),
    /// Body length did not match the manifest's `size` field.
    SizeMismatch { expected: u64, actual: u64 },
    /// sha256 of the downloaded bytes did not match the manifest.
    HashMismatch { expected: String, actual: String },
    /// Caller requested cancellation while streaming the body.
    Canceled,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Io(error) => write!(f, "io: {error}"),
            DownloadError::Network(error) => write!(f, "network: {error}"),
            DownloadError::SizeMismatch { expected, actual } => {
                write!(f, "size mismatch: expected {expected}, got {actual}")
            }
            DownloadError::HashMismatch { expected, actual } => {
                write!(f, "sha256 mismatch: expected {expected}, got {actual}")
            }
            DownloadError::Canceled => write!(f, "canceled"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<io::Error> for DownloadError {
    fn from(error: io::Error) -> Self {
        DownloadError::Io(error)
    }
}

/// Progress callback signature. Receives a freshly observed
/// `(bytes_downloaded, total_bytes)` pair on each chunk boundary; the
/// caller is responsible for rate-limiting if it cares. Total is taken
/// from the manifest, not from the HTTP response, so progress is honest
/// even if the server omits Content-Length.
pub(crate) type ProgressCallback<'a> = dyn FnMut(u64, u64) + 'a;

/// Stream `source` to `destination`, calling `progress` after each chunk
/// and verifying the manifest's sha256 + size when the body ends.
///
/// On any failure the destination file is removed before returning so a
/// half-written partial download cannot be mistaken for a verified zip
/// on the next launch.
pub(crate) fn download_from_reader_cancellable(
    source: &mut dyn Read,
    destination: &Path,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut ProgressCallback<'_>,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(), DownloadError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    // Truncate any prior partial so a previous crash cannot leak bytes
    // into the new download. The cleanup-on-failure path also relies on
    // this file existing as a sentinel.
    let mut file = File::create(destination)?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; READ_CHUNK];
    let mut written: u64 = 0;
    // Emit a 0/total tick so callers can paint an initial bar without
    // waiting for the first chunk to land.
    progress(0, expected_size);

    loop {
        if should_cancel() {
            drop(file);
            let _ = fs::remove_file(destination);
            return Err(DownloadError::Canceled);
        }
        let read = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(destination);
                return Err(DownloadError::Network(error.to_string()));
            }
        };
        if let Err(error) = file.write_all(&buffer[..read]) {
            drop(file);
            let _ = fs::remove_file(destination);
            return Err(DownloadError::Io(error));
        }
        hasher.update(&buffer[..read]);
        written += read as u64;
        progress(written, expected_size);
    }
    if should_cancel() {
        drop(file);
        let _ = fs::remove_file(destination);
        return Err(DownloadError::Canceled);
    }

    if let Err(error) = file.flush() {
        drop(file);
        let _ = fs::remove_file(destination);
        return Err(DownloadError::Io(error));
    }
    drop(file);

    if written != expected_size {
        let _ = fs::remove_file(destination);
        return Err(DownloadError::SizeMismatch {
            expected: expected_size,
            actual: written,
        });
    }

    let actual = hex::encode(hasher.finalize());
    // Manifest hex casing is the publisher's choice.
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        let _ = fs::remove_file(destination);
        return Err(DownloadError::HashMismatch {
            expected: expected_sha256.to_owned(),
            actual,
        });
    }

    Ok(())
}

/// Production entry point: fetch `url` over HTTPS and delegate to
/// [`download_from_reader_cancellable`].
pub(crate) fn download_from_url(
    url: &str,
    destination: &Path,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut ProgressCallback<'_>,
) -> Result<(), DownloadError> {
    download_from_url_cancellable(
        url,
        destination,
        expected_size,
        expected_sha256,
        progress,
        &|| false,
    )
}

pub(crate) fn download_from_url_cancellable(
    url: &str,
    destination: &Path,
    expected_size: u64,
    expected_sha256: &str,
    progress: &mut ProgressCallback<'_>,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(), DownloadError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|error| DownloadError::Network(format!("client: {error}")))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| DownloadError::Network(format!("send: {error}")))?;
    if !response.status().is_success() {
        return Err(DownloadError::Network(format!(
            "status: {}",
            response.status()
        )));
    }
    let mut reader = BufReader::new(response);
    download_from_reader_cancellable(
        &mut reader,
        destination,
        expected_size,
        expected_sha256,
        progress,
        should_cancel,
    )
}

/// Compute the sha256 of `bytes`, returned as lowercase hex for test fixtures.
#[cfg(test)]
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::channel;
    use std::thread;

    fn serve_once(payload: Vec<u8>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/zip");
        let (ready_tx, ready_rx) = channel::<()>();
        let handle = thread::spawn(move || {
            ready_tx.send(()).unwrap();
            if let Ok((stream, _)) = listener.accept() {
                handle_request(stream, &payload);
            }
        });
        ready_rx.recv().unwrap();
        (url, handle)
    }

    fn handle_request(mut stream: TcpStream, payload: &[u8]) {
        // Read the HTTP request line + headers. We don't actually care
        // about the contents, just need to consume them before responding
        // so the client's send() returns.
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/zip\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        let _ = stream.write_all(headers.as_bytes());
        let _ = stream.write_all(payload);
        let _ = stream.flush();
    }

    #[test]
    fn download_from_reader_emits_progress_during_download() {
        let dir = TempRoot::new("progress");
        // Big enough to span multiple READ_CHUNK reads.
        let payload = vec![0u8; READ_CHUNK * 3 + 7];
        let expected = sha256_hex(&payload);
        let destination = dir.join("progress.zip");

        let mut source = &payload[..];
        let mut ticks: Vec<(u64, u64)> = Vec::new();
        let mut progress = |done, total| ticks.push((done, total));
        download_from_reader_cancellable(
            &mut source,
            &destination,
            payload.len() as u64,
            &expected,
            &mut progress,
            &|| false,
        )
        .unwrap();

        // At least: initial 0-tick + one per chunk + final equality.
        assert!(
            ticks.len() >= 3,
            "expected multiple progress ticks, got {ticks:?}",
        );
        assert_eq!(ticks.first(), Some(&(0, payload.len() as u64)));
        assert_eq!(
            ticks.last(),
            Some(&(payload.len() as u64, payload.len() as u64)),
        );
        // Progress must be monotonically non-decreasing.
        for window in ticks.windows(2) {
            assert!(
                window[1].0 >= window[0].0,
                "progress went backwards: {ticks:?}",
            );
        }
    }

    #[test]
    fn download_from_reader_rejects_sha_mismatch_and_cleans_up() {
        let dir = TempRoot::new("hash-mismatch");
        let payload = b"valid-bytes".to_vec();
        let wrong = "0".repeat(64);
        let destination = dir.join("bad.zip");

        let mut source = &payload[..];
        let mut progress = |_, _| {};
        let err = download_from_reader_cancellable(
            &mut source,
            &destination,
            payload.len() as u64,
            &wrong,
            &mut progress,
            &|| false,
        )
        .unwrap_err();

        match err {
            DownloadError::HashMismatch { expected, actual } => {
                assert_eq!(expected, wrong);
                assert_eq!(actual, sha256_hex(&payload));
            }
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        assert!(
            !destination.exists(),
            "verified-bad download must not leave bytes on disk",
        );
    }

    #[test]
    fn download_from_reader_rejects_size_mismatch_and_cleans_up() {
        let dir = TempRoot::new("size-mismatch");
        let payload = b"too-short".to_vec();
        let expected = sha256_hex(&payload);
        let destination = dir.join("short.zip");

        let mut source = &payload[..];
        let mut progress = |_, _| {};
        let err = download_from_reader_cancellable(
            &mut source,
            &destination,
            payload.len() as u64 + 10,
            &expected,
            &mut progress,
            &|| false,
        )
        .unwrap_err();

        match err {
            DownloadError::SizeMismatch { expected, actual } => {
                assert_eq!(expected, payload.len() as u64 + 10);
                assert_eq!(actual, payload.len() as u64);
            }
            other => panic!("expected SizeMismatch, got {other:?}"),
        }
        assert!(!destination.exists());
    }

    #[test]
    fn download_from_url_against_local_http_fixture_succeeds_on_match() {
        let dir = TempRoot::new("http-ok");
        let payload = b"http-good-bytes".to_vec();
        let expected = sha256_hex(&payload);
        let destination = dir.join("served.zip");

        let (url, handle) = serve_once(payload.clone());
        let mut progress = |_, _| {};
        download_from_url(
            &url,
            &destination,
            payload.len() as u64,
            &expected,
            &mut progress,
        )
        .unwrap();
        let _ = handle.join();

        assert_eq!(fs::read(&destination).unwrap(), payload);
    }

    #[test]
    fn hash_compare_is_case_insensitive() {
        let bytes = b"abc";
        let lower = sha256_hex(bytes);
        let upper = lower.to_uppercase();
        let dir = TempRoot::new("case");
        let dest = dir.join("case.zip");
        let mut source = &bytes[..];
        let mut progress = |_, _| {};
        download_from_reader_cancellable(
            &mut source,
            &dest,
            bytes.len() as u64,
            &upper,
            &mut progress,
            &|| false,
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), bytes);
    }
}
