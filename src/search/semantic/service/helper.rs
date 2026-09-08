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

impl EmbeddingHelperClient {
    fn new() -> Result<(Self, ModelLoadTiming), String> {
        let executable = std::env::current_exe()
            .map_err(|error| format!("failed to find app executable: {error}"))?;
        let mut child = Command::new(executable)
            .arg("--semantic-embed-server")
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
    if helper.is_none() {
        let (client, _) = EmbeddingHelperClient::new()?;
        *helper = Some(client);
    }
    helper
        .as_mut()
        .ok_or_else(|| "embedding helper unavailable".to_owned())?
        .embed_image(source_path)
}

pub(super) fn ensure_embedding_helper(
    helper: &mut Option<EmbeddingHelperClient>,
) -> Result<Option<ModelLoadTiming>, String> {
    if helper.is_none() {
        let (client, timing) = EmbeddingHelperClient::new()?;
        *helper = Some(client);
        Ok(Some(timing))
    } else {
        Ok(None)
    }
}
