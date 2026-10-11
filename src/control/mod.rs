//! App control lets a local AI client drive the running app.
//!
//! `snappyref --mcp` speaks MCP on stdio and forwards each tool call over a
//! loopback connection to the GUI process, which runs it on the UI thread.
//! The GUI process stays the only writer of the Library.

mod mcp;
mod tools;

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use eframe::egui;
use serde::{Deserialize, Serialize};

pub(crate) use mcp::run_mcp_stdio;
#[cfg(test)]
pub(crate) use tools::tool_names;

/// Where a running app with AI control on publishes its listener. The file
/// outlives the app, so `--mcp` reads its presence as "access is on" and
/// launches the app when nothing answers.
const ENDPOINT_FILE: &str = "control.json";

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
}

#[derive(Deserialize)]
struct Call {
    tool: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

/// One tool call waiting for the UI thread.
pub(crate) struct ControlRequest {
    pub(crate) tool: String,
    pub(crate) arguments: serde_json::Value,
    pub(crate) responder: Responder,
}

/// Answers one tool call. Dropping it unanswered reports a failure.
pub(crate) struct Responder(mpsc::Sender<ControlReply>);

impl Responder {
    pub(crate) fn send(self, reply: ControlReply) {
        let _ = self.0.send(reply);
    }
}

pub(crate) struct ControlReply {
    pub(crate) result: Result<serde_json::Value, String>,
    pub(crate) png: Option<Vec<u8>>,
}

impl ControlReply {
    pub(crate) fn from_result(result: Result<serde_json::Value, String>) -> Self {
        Self { result, png: None }
    }

    /// The MCP `tools/call` result the client receives.
    fn into_tool_result(self) -> serde_json::Value {
        use base64::Engine as _;
        match self.result {
            Ok(value) => {
                let mut content = Vec::new();
                if !value.is_null() {
                    content.push(serde_json::json!({"type": "text", "text": value.to_string()}));
                }
                if let Some(png) = self.png {
                    content.push(serde_json::json!({
                        "type": "image",
                        "data": base64::engine::general_purpose::STANDARD.encode(png),
                        "mimeType": "image/png",
                    }));
                }
                serde_json::json!({"content": content, "isError": false})
            }
            Err(message) => tool_error(&message),
        }
    }
}

fn tool_error(message: &str) -> serde_json::Value {
    serde_json::json!({"content": [{"type": "text", "text": message}], "isError": true})
}

/// The loopback listener of a running app. Requests cross to the UI thread
/// through `try_recv`.
pub(crate) struct ControlServer {
    requests: mpsc::Receiver<ControlRequest>,
    port: u16,
    stopped: Arc<AtomicBool>,
}

impl ControlServer {
    pub(crate) fn start(library_root: &Path, ctx: egui::Context) -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("Failed to open the AI control listener: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Failed to read the AI control port: {error}"))?
            .port();
        let mut secret = [0u8; 16];
        getrandom::fill(&mut secret)
            .map_err(|error| format!("Failed to create the AI control token: {error}"))?;
        let token = hex::encode(secret);
        write_endpoint(
            &library_root.join(ENDPOINT_FILE),
            &Endpoint {
                port,
                token: token.clone(),
            },
        )?;

        let (sender, requests) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let accept_stopped = stopped.clone();
        std::thread::Builder::new()
            .name("control-listener".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accept_stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    let Ok(stream) = stream else { continue };
                    let (token, sender, ctx) = (token.clone(), sender.clone(), ctx.clone());
                    let _ = std::thread::Builder::new()
                        .name("control-connection".to_owned())
                        .spawn(move || serve_connection(stream, &token, &sender, &ctx));
                }
            })
            .map_err(|error| format!("Failed to start the AI control listener: {error}"))?;
        Ok(Self {
            requests,
            port,
            stopped,
        })
    }

    pub(crate) fn try_recv(&self) -> Option<ControlRequest> {
        self.requests.try_recv().ok()
    }
}

/// Turning access off withdraws the endpoint, so `--mcp` stops launching the
/// app.
pub(crate) fn withdraw(library_root: &Path) {
    let _ = std::fs::remove_file(library_root.join(ENDPOINT_FILE));
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        // Wakes the accept loop so it sees the flag.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
    }
}

fn write_endpoint(path: &Path, endpoint: &Endpoint) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("Failed to write {}: {error}", path.display());
    let json = serde_json::to_vec(endpoint).map_err(|error| error.to_string())?;
    // A stale file may have looser permissions than a new one gets.
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut file| file.write_all(&json))
        .map_err(failed)
}

/// One call the way `--mcp` sends it, without the stdio side.
#[cfg(test)]
pub(crate) fn call_running_app(
    library_root: &Path,
    call: &serde_json::Value,
) -> Option<serde_json::Value> {
    mcp::AppConnection::connect(&read_endpoint(library_root)?)?.call(call)
}

fn read_endpoint(library_root: &Path) -> Option<Endpoint> {
    let bytes = std::fs::read(library_root.join(ENDPOINT_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The token line comes first. Each later line is one call, answered by one line.
fn serve_connection(
    stream: TcpStream,
    token: &str,
    requests: &mpsc::Sender<ControlRequest>,
    ctx: &egui::Context,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() || line.trim_end() != token {
        return;
    }
    if writer.write_all(b"ok\n").is_err() {
        return;
    }
    loop {
        line.clear();
        if !matches!(reader.read_line(&mut line), Ok(read) if read > 0) {
            return;
        }
        let result = match serde_json::from_str::<Call>(&line) {
            Ok(call) => {
                let (sender, reply) = mpsc::channel();
                let request = ControlRequest {
                    tool: call.tool,
                    arguments: call.arguments,
                    responder: Responder(sender),
                };
                if requests.send(request).is_err() {
                    return;
                }
                ctx.request_repaint();
                match reply.recv() {
                    Ok(reply) => reply.into_tool_result(),
                    Err(_) => tool_error("SnappyRef dropped the request."),
                }
            }
            Err(error) => tool_error(&format!("Malformed call: {error}")),
        };
        if writeln!(writer, "{result}").is_err() {
            return;
        }
    }
}
