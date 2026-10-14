//! Exercise the MCP process boundary without starting the GUI.
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[path = "../src/test_support.rs"]
mod test_support;
use test_support::TempRoot;

fn run_mcp(root: &std::path::Path, input: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_snappyref"))
        .arg("--mcp")
        .env("SNAPPYREF_LIBRARY_ROOT", root)
        .env("SNAPPYREF_DATA_ROOT", root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "MCP did not exit on EOF: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn stdio_frames_requests_batches_and_parse_errors_and_exits_on_eof() {
    let root = TempRoot::new("mcp-stdio");
    let output = run_mcp(
        &root,
        concat!(
            "\n  \n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{broken\n",
            "[{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"},{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"id\":\"bad\",\"method\":\"missing\"}]\n",
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n"
        ),
    );
    assert!(output.ends_with('\n'));
    let lines: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["jsonrpc"], "2.0");
    assert_eq!(lines[0]["id"], Value::Null);
    assert_eq!(lines[0]["error"]["code"], -32700);
    assert!(
        lines[0]["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Parse error: ")
    );
    assert_eq!(
        lines[1],
        json!([
            {"jsonrpc":"2.0", "id":7, "result":{}},
            {"jsonrpc":"2.0", "id":"bad", "error":{"code":-32601,"message":"Method not found: missing"}}
        ])
    );
    assert_eq!(lines[2], json!({"jsonrpc":"2.0", "id":9, "result":{}}));
}

#[test]
fn lost_app_reply_reports_ambiguous_result_without_replaying_the_edit() {
    let root = TempRoot::new("mcp-lost-reply");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    std::fs::write(
        root.join("control.json"),
        json!({
            "port":listener.local_addr().unwrap().port(), "token":"test-secret"
        })
        .to_string(),
    )
    .unwrap();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "test-secret\n");
        stream.write_all(b"ok\n").unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({
                "tool":"edit_board", "arguments":{"board":7,"edits":[]}
            })
        );
        drop(reader);
        drop(stream);
        listener
    });
    let output = run_mcp(
        &root,
        "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"tools/call\",\"params\":{\"name\":\"edit_board\",\"arguments\":{\"board\":7,\"edits\":[]}}}\n",
    );
    assert_eq!(output.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap(),
        json!({
            "jsonrpc":"2.0", "id":11, "result":{
                "isError":true, "content":[{"type":"text","text":"SnappyRef did not answer. The call may have run, so read the board before you send it again."}]
            }
        })
    );
    let listener = peer.join().unwrap();
    listener.set_nonblocking(true).unwrap();
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}
