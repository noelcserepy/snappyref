use super::super::model::{DEFAULT_SEMANTIC_SEARCH_MODEL, ModelLoadTiming, OrtImageEmbeddingModel};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(serde::Deserialize, serde::Serialize)]
struct EmbedImageOutput {
    embedding: Option<Vec<f32>>,
    error: Option<String>,
    model_load_timing: Option<EmbedModelLoadTimingOutput>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct EmbedModelLoadTimingOutput {
    assets_resolved_ms: f64,
    tokenizer_loaded_ms: f64,
    text_session_loaded_ms: f64,
    vision_session_loaded_ms: f64,
    total_ms: f64,
}

impl From<ModelLoadTiming> for EmbedModelLoadTimingOutput {
    fn from(timing: ModelLoadTiming) -> Self {
        Self {
            assets_resolved_ms: timing.assets_resolved.as_secs_f64() * 1000.0,
            tokenizer_loaded_ms: timing.tokenizer_loaded.as_secs_f64() * 1000.0,
            text_session_loaded_ms: timing.text_session_loaded.as_secs_f64() * 1000.0,
            vision_session_loaded_ms: timing.vision_session_loaded.as_secs_f64() * 1000.0,
            total_ms: timing.total.as_secs_f64() * 1000.0,
        }
    }
}

impl From<EmbedModelLoadTimingOutput> for ModelLoadTiming {
    fn from(timing: EmbedModelLoadTimingOutput) -> Self {
        Self {
            assets_resolved: std::time::Duration::from_secs_f64(timing.assets_resolved_ms / 1000.0),
            tokenizer_loaded: std::time::Duration::from_secs_f64(
                timing.tokenizer_loaded_ms / 1000.0,
            ),
            text_session_loaded: std::time::Duration::from_secs_f64(
                timing.text_session_loaded_ms / 1000.0,
            ),
            vision_session_loaded: std::time::Duration::from_secs_f64(
                timing.vision_session_loaded_ms / 1000.0,
            ),
            total: std::time::Duration::from_secs_f64(timing.total_ms / 1000.0),
        }
    }
}

pub(crate) fn run_embed_image_server() -> Result<(), String> {
    let (model, timing) =
        OrtImageEmbeddingModel::load_with_progress_timed(DEFAULT_SEMANTIC_SEARCH_MODEL, |_| {})
            .map_err(|error| {
                format!("failed to load semantic model in embedding helper: {error}")
            })?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(
        &mut stdout,
        &EmbedImageOutput {
            embedding: None,
            error: None,
            model_load_timing: Some(timing.into()),
        },
    )
    .map_err(|error| format!("failed to write embedding helper ready output: {error}"))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| format!("failed to finish embedding helper ready output: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush embedding helper ready output: {error}"))?;
    for line in stdin.lock().lines() {
        let line =
            line.map_err(|error| format!("failed to read embedding helper input: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let path: std::path::PathBuf = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper input: {error}"))?;
        let output = match model.embed_image_path_timed(&path) {
            Ok((embedding, _timing)) => EmbedImageOutput {
                embedding: Some(embedding),
                error: None,
                model_load_timing: None,
            },
            Err(error) => EmbedImageOutput {
                embedding: None,
                error: Some(format!(
                    "failed to embed image in embedding helper: {error}"
                )),
                model_load_timing: None,
            },
        };
        serde_json::to_writer(&mut stdout, &output)
            .map_err(|error| format!("failed to write embedding helper output: {error}"))?;
        stdout
            .write_all(b"\n")
            .map_err(|error| format!("failed to finish embedding helper output: {error}"))?;
        stdout
            .flush()
            .map_err(|error| format!("failed to flush embedding helper output: {error}"))?;
    }
    Ok(())
}
pub(super) struct EmbeddingHelperClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// Which process the index worker starts as its Embedding Helper Process.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum EmbeddingHelperLaunch {
    /// This executable in its `--semantic-embed-server` role.
    #[default]
    App,
    /// An offline test helper that speaks the same protocol at roughly the
    /// real pace and answers every image with the first unit vector.
    #[cfg(test)]
    Fixture,
}

impl EmbeddingHelperClient {
    fn new(launch: EmbeddingHelperLaunch) -> Result<(Self, ModelLoadTiming), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to find app executable: {error}"))?;
        let mut command = Command::new(executable);
        match launch {
            EmbeddingHelperLaunch::App => command.arg("--semantic-embed-server"),
            #[cfg(test)]
            EmbeddingHelperLaunch::Fixture => tests::fixture_command(&mut command, "serve"),
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to run embedding helper: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "embedding helper stdin unavailable".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "embedding helper stdout unavailable".to_owned())?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        #[cfg(test)]
        if matches!(launch, EmbeddingHelperLaunch::Fixture) {
            tests::skip_test_runner_header(&mut client.stdout);
        }
        let timing = client.read_ready_timing()?;
        Ok((client, timing))
    }

    fn read_ready_timing(&mut self) -> Result<ModelLoadTiming, String> {
        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read embedding helper ready output: {error}"))?;
        if bytes == 0 {
            let status = self.child.try_wait().ok().flatten();
            return Err(match status {
                Some(status) => {
                    format!("embedding helper exited before ready output with {status}")
                }
                None => "embedding helper closed stdout before ready output".to_owned(),
            });
        }
        let output: EmbedImageOutput = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper ready output: {error}"))?;
        if let Some(error) = output.error {
            return Err(error);
        }
        output
            .model_load_timing
            .map(Into::into)
            .ok_or_else(|| "embedding helper ready output missing model timing".to_owned())
    }

    fn embed_image(&mut self, source_path: &PathBuf) -> Result<Vec<f32>, String> {
        serde_json::to_writer(&mut self.stdin, source_path)
            .map_err(|error| format!("failed to write embedding helper input: {error}"))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|error| format!("failed to finish embedding helper input: {error}"))?;
        self.stdin
            .flush()
            .map_err(|error| format!("failed to flush embedding helper input: {error}"))?;

        let mut line = String::new();
        let bytes = self
            .stdout
            .read_line(&mut line)
            .map_err(|error| format!("failed to read embedding helper output: {error}"))?;
        if bytes == 0 {
            let status = self.child.try_wait().ok().flatten();
            return Err(match status {
                Some(status) => format!("embedding helper exited before output with {status}"),
                None => "embedding helper closed stdout before output".to_owned(),
            });
        }
        let output: EmbedImageOutput = serde_json::from_str(&line)
            .map_err(|error| format!("failed to decode embedding helper output: {error}"))?;
        if let Some(error) = output.error {
            return Err(error);
        }
        output
            .embedding
            .ok_or_else(|| "embedding helper output missing embedding".to_owned())
    }
}

impl Drop for EmbeddingHelperClient {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) fn embed_image_in_helper(
    source_path: &PathBuf,
    helper: &mut Option<EmbeddingHelperClient>,
) -> Result<Vec<f32>, String> {
    helper
        .as_mut()
        .ok_or_else(|| "embedding helper unavailable".to_owned())?
        .embed_image(source_path)
}

pub(super) fn ensure_embedding_helper(
    helper: &mut Option<EmbeddingHelperClient>,
    launch: EmbeddingHelperLaunch,
) -> Result<Option<ModelLoadTiming>, String> {
    if helper.is_none() {
        let (client, timing) = EmbeddingHelperClient::new(launch)?;
        *helper = Some(client);
        Ok(Some(timing))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_ENV: &str = "SNAPPYREF_EMBED_PROTOCOL_FIXTURE";
    const READY: &str = r#"{"model_load_timing":{"assets_resolved_ms":1,"tokenizer_loaded_ms":2,"text_session_loaded_ms":3,"vision_session_loaded_ms":4,"total_ms":10}}"#;

    pub(super) fn fixture_command<'a>(command: &'a mut Command, scenario: &str) -> &'a mut Command {
        command
            .args([
                "--exact",
                "search::semantic::service::helper::tests::protocol_fixture_child",
                "--ignored",
                "--nocapture",
            ])
            .env(FIXTURE_ENV, scenario)
    }

    /// The test runner prints a header before the fixture owns stdout.
    pub(super) fn skip_test_runner_header(stdout: &mut BufReader<ChildStdout>) {
        loop {
            let mut line = String::new();
            assert_ne!(
                stdout.read_line(&mut line).unwrap(),
                0,
                "fixture did not start"
            );
            if line.trim() == "embedding-protocol-fixture" {
                break;
            }
        }
    }

    fn fixture(scenario: &str) -> EmbeddingHelperClient {
        let mut child = fixture_command(
            &mut Command::new(std::env::current_exe().unwrap()),
            scenario,
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        skip_test_runner_header(&mut stdout);
        EmbeddingHelperClient {
            child,
            stdin,
            stdout,
        }
    }

    #[test]
    #[ignore = "spawned by the offline embedding protocol tests"]
    fn protocol_fixture_child() {
        let scenario = std::env::var(FIXTURE_ENV).expect("fixture scenario");
        if scenario == "serve" {
            serve_fixture_embeddings();
        }
        // A missing request newline or flush must fail rather than hang CI.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(10));
            std::process::exit(86);
        });
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "embedding-protocol-fixture").unwrap();
        stdout.flush().unwrap();
        match scenario.as_str() {
            "exit-ready" => std::process::exit(0),
            "malformed-ready" => writeln!(stdout, "not json").unwrap(),
            "missing-ready" => writeln!(stdout, "{{}}").unwrap(),
            _ => writeln!(stdout, "{READY}").unwrap(),
        }
        stdout.flush().unwrap();
        if scenario.ends_with("ready") {
            std::process::exit(0);
        }
        let mut stdin = std::io::stdin().lock();
        let mut request = String::new();
        stdin.read_line(&mut request).unwrap();
        match scenario.as_str() {
            "consecutive" => {
                assert_eq!(request, "\"folder/quoted\\\"name\\\\line\\n.png\"\n");
                writeln!(stdout, r#"{{"embedding":[0.25,-0.5]}}"#).unwrap();
                stdout.flush().unwrap();
                request.clear();
                stdin.read_line(&mut request).unwrap();
                assert_eq!(request, "\"second image.png\"\n");
                writeln!(stdout, r#"{{"embedding":[1.0,0.0]}}"#).unwrap();
            }
            "malformed-result" => writeln!(stdout, "not json").unwrap(),
            "missing-result" => writeln!(stdout, "{{}}").unwrap(),
            "error-result" => {
                writeln!(stdout, r#"{{"error":"image could not be decoded"}}"#).unwrap()
            }
            "exit-result" => std::process::exit(0),
            _ => panic!("unknown fixture scenario"),
        }
        stdout.flush().unwrap();
        std::process::exit(0);
    }

    /// `EmbeddingHelperLaunch::Fixture`: loads and embeds at roughly the real
    /// helper's pace, until the index worker closes stdin.
    fn serve_fixture_embeddings() -> ! {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "embedding-protocol-fixture").unwrap();
        stdout.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        writeln!(stdout, "{READY}").unwrap();
        stdout.flush().unwrap();
        let mut embedding = vec![0.0_f32; DEFAULT_SEMANTIC_SEARCH_MODEL.dimension];
        embedding[0] = 1.0;
        let answer = serde_json::json!({ "embedding": embedding }).to_string();
        for _request in std::io::stdin().lock().lines().map_while(Result::ok) {
            std::thread::sleep(std::time::Duration::from_millis(150));
            writeln!(stdout, "{answer}").unwrap();
            stdout.flush().unwrap();
        }
        std::process::exit(0);
    }

    #[test]
    fn helper_sends_escaped_paths_and_reads_consecutive_embeddings() {
        let mut client = fixture("consecutive");
        let timing = client.read_ready_timing().unwrap();
        assert_eq!(timing.assets_resolved, std::time::Duration::from_millis(1));
        assert_eq!(timing.tokenizer_loaded, std::time::Duration::from_millis(2));
        assert_eq!(
            timing.text_session_loaded,
            std::time::Duration::from_millis(3)
        );
        assert_eq!(
            timing.vision_session_loaded,
            std::time::Duration::from_millis(4)
        );
        assert_eq!(timing.total, std::time::Duration::from_millis(10));
        assert_eq!(
            client.embed_image(&PathBuf::from("folder/quoted\"name\\line\n.png")),
            Ok(vec![0.25, -0.5])
        );
        assert_eq!(
            client.embed_image(&PathBuf::from("second image.png")),
            Ok(vec![1.0, 0.0])
        );
    }

    #[test]
    fn helper_rejects_malformed_or_incomplete_protocol_messages() {
        for (scenario, expected) in [
            (
                "malformed-ready",
                "failed to decode embedding helper ready output: expected ident at line 1 column 2",
            ),
            (
                "missing-ready",
                "embedding helper ready output missing model timing",
            ),
        ] {
            let mut client = fixture(scenario);
            assert_eq!(client.read_ready_timing().err().as_deref(), Some(expected));
        }
        for (scenario, expected) in [
            (
                "malformed-result",
                "failed to decode embedding helper output: expected ident at line 1 column 2",
            ),
            (
                "missing-result",
                "embedding helper output missing embedding",
            ),
            ("error-result", "image could not be decoded"),
        ] {
            let mut client = fixture(scenario);
            client.read_ready_timing().unwrap();
            assert_eq!(
                client.embed_image(&PathBuf::from("image.png")),
                Err(expected.to_owned())
            );
        }
    }

    #[test]
    fn helper_reports_child_exit_before_ready_or_result() {
        let mut client = fixture("exit-ready");
        // Reap first so the exit status is deterministic rather than a race
        // between closed stdout and try_wait observing process termination.
        let status = client.child.wait().unwrap();
        assert!(status.success());
        assert_eq!(
            client.read_ready_timing().err(),
            Some(format!(
                "embedding helper exited before ready output with {status}"
            ))
        );

        let mut client = fixture("exit-result");
        client.read_ready_timing().unwrap();
        let error = client.embed_image(&PathBuf::from("image.png")).unwrap_err();
        let status = client.child.wait().unwrap();
        assert!(status.success());
        assert!(
            error == format!("embedding helper exited before output with {status}")
                || error == "embedding helper closed stdout before output",
            "{error}"
        );
    }
}
