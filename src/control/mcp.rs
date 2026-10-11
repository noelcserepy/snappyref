//! The `--mcp` role: an MCP server on stdio that forwards tool calls to the
//! running app, launching it first when AI control is on and nothing answers.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Endpoint, read_endpoint, tool_error, tools};

const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const ACCESS_OFF: &str = "AI control is off in SnappyRef. Ask the user to open SnappyRef, go to \
    Settings > General > AI control and choose Ask or Full access, then try again.";
const LAUNCH_WAIT: Duration = Duration::from_secs(30);
/// Longer than the app lets any tool wait.
const CALL_WAIT: Duration = Duration::from_secs(150);
const NO_ANSWER: &str = "SnappyRef did not answer. The call may have run, so read the board \
    before you send it again.";

pub(crate) fn run_mcp_stdio() -> Result<(), String> {
    let library_root = crate::local_data_location::resolve_library_root()?;
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|error| format!("Failed to read stdin: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let answers = batch
                    .iter()
                    .filter_map(|message| answer(message, &library_root))
                    .collect::<Vec<_>>();
                (!answers.is_empty()).then_some(Value::Array(answers))
            }
            Ok(message) => answer(&message, &library_root),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {error}")},
            })),
        };
        if let Some(response) = response {
            writeln!(stdout, "{response}")
                .and_then(|()| stdout.flush())
                .map_err(|error| format!("Failed to write stdout: {error}"))?;
        }
    }
    Ok(())
}

/// Notifications get no answer.
fn answer(message: &Value, library_root: &Path) -> Option<Value> {
    let id = message.get("id")?.clone();
    let params = message.get("params");
    let result = match message.get("method").and_then(Value::as_str)? {
        "initialize" => {
            let requested = params
                .and_then(|params| params.get("protocolVersion"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let version = PROTOCOL_VERSIONS
                .into_iter()
                .find(|version| *version == requested)
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "snappyref", "version": env!("CARGO_PKG_VERSION")},
                "instructions": tools::INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::catalogue()})),
        "tools/call" => Ok(call_tool(params, library_root)),
        method => Err(json!({"code": -32601, "message": format!("Method not found: {method}")})),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

/// A call that was sent is never sent again. The app may have run it before
/// the connection broke, and a second run would repeat its edits.
fn call_tool(params: Option<&Value>, library_root: &Path) -> Value {
    let call = json!({
        "tool": params.and_then(|params| params.get("name")).cloned().unwrap_or_default(),
        "arguments": params.and_then(|params| params.get("arguments")).cloned().unwrap_or_default(),
    });
    match AppConnection::open(library_root) {
        Ok(connection) => connection
            .call(&call)
            .unwrap_or_else(|| tool_error(NO_ANSWER)),
        Err(error) => tool_error(&error),
    }
}

/// A connection the app took the token on. It carries one call.
pub(super) struct AppConnection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl AppConnection {
    fn open(library_root: &Path) -> Result<Self, String> {
        let Some(endpoint) = read_endpoint(library_root) else {
            return Err(ACCESS_OFF.to_owned());
        };
        if let Some(connection) = Self::connect(&endpoint) {
            return Ok(connection);
        }
        launch_app()?;
        let deadline = Instant::now() + LAUNCH_WAIT;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
            // The launched app publishes a new port and token.
            match read_endpoint(library_root) {
                Some(endpoint) => {
                    if let Some(connection) = Self::connect(&endpoint) {
                        return Ok(connection);
                    }
                }
                None => return Err(ACCESS_OFF.to_owned()),
            }
        }
        Err("Started SnappyRef, but it did not answer in time. Try again.".to_owned())
    }

    pub(super) fn connect(endpoint: &Endpoint) -> Option<Self> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
        let mut writer = TcpStream::connect_timeout(&address, Duration::from_millis(500)).ok()?;
        let mut reader = BufReader::new(writer.try_clone().ok()?);
        // Something else may own a stale port. It proves nothing about who
        // listens, but only the app answers "ok" to this token.
        writer.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        writeln!(writer, "{}", endpoint.token).ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line.trim_end() != "ok" {
            return None;
        }
        writer.set_read_timeout(Some(CALL_WAIT)).ok()?;
        Some(Self { reader, writer })
    }

    /// None when the connection broke or the app took too long.
    pub(super) fn call(mut self, call: &Value) -> Option<Value> {
        writeln!(self.writer, "{call}").ok()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        serde_json::from_str(&line).ok()
    }
}

/// Starts the GUI as its own process so it outlives this MCP session. It
/// inherits the environment, so a data root override carries over. The user is
/// working in another app when a client calls, so the window opens behind it.
fn launch_app() -> Result<(), String> {
    let exe =
        std::env::current_exe().map_err(|error| format!("Failed to locate SnappyRef: {error}"))?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("--no-startup-focus")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
        .spawn()
        .map(drop)
        .map_err(|error| format!("Failed to start SnappyRef: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_handshake_lists_tools_without_the_app() {
        let root = crate::test_support::TempRoot::new("control-mcp");
        let initialize = answer(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
            &root,
        )
        .unwrap();
        assert_eq!(initialize["result"]["protocolVersion"], "2025-06-18");
        assert!(
            answer(
                &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                &root,
            )
            .is_none()
        );
        let listed = answer(
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            &root,
        )
        .unwrap();
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "edit_board"));

        // No endpoint file means access is off, so a call explains how to turn it on
        // and never launches the app.
        let called = answer(
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "get_board"}}),
            &root,
        )
        .unwrap();
        assert_eq!(called["result"]["isError"], true);
        assert_eq!(called["result"]["content"][0]["text"], ACCESS_OFF);
    }

    #[test]
    fn call_reaches_the_app_only_with_its_token() {
        let root = crate::test_support::TempRoot::new("control-connection");
        let server =
            crate::control::ControlServer::start(&root, eframe::egui::Context::default()).unwrap();
        let endpoint = read_endpoint(&root).unwrap();
        let stranger = Endpoint {
            port: endpoint.port,
            token: "guess".to_owned(),
        };
        assert!(AppConnection::connect(&stranger).is_none());

        let connection = AppConnection::connect(&endpoint).unwrap();
        let ui_thread = std::thread::spawn(move || {
            loop {
                if let Some(request) = server.try_recv() {
                    let echo = json!({"tool": request.tool, "arguments": request.arguments});
                    request
                        .responder
                        .send(crate::control::ControlReply::from_result(Ok(echo)));
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        let call = json!({"tool": "open_board", "arguments": {"board": 7}});
        let result = connection.call(&call).unwrap();
        ui_thread.join().unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["text"], call.to_string());
    }
}
