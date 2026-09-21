//! Transport E2E: spawn the real `sigil-lsp` binary over stdio and speak
//! the LSP wire format (Content-Length framing) by hand.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use common::TempDir;

mod common;

struct Proc {
    child: Child,
    writer: std::process::ChildStdin,
    reader_thread: Option<JoinHandle<Vec<serde_json::Value>>>,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start() -> Proc {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sigil-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn sigil-lsp");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // Reader thread: frame-parse stdout into raw JSON values.
    let reader_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut messages = Vec::new();
        loop {
            let mut content_length: Option<usize> = None;
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => return messages, // EOF
                    Ok(_) => {
                        let line = line.trim_end();
                        if line.is_empty() {
                            break; // end of headers
                        }
                        if let Some(v) = line
                            .strip_prefix("Content-Length:")
                            .map(|v| v.trim().parse().unwrap())
                        {
                            content_length = Some(v);
                        }
                    }
                    Err(_) => return messages,
                }
            }
            let Some(len) = content_length else {
                return messages;
            };
            let mut buf = vec![0u8; len];
            if reader.read_exact(&mut buf).is_err() {
                return messages;
            }
            match serde_json::from_slice::<serde_json::Value>(&buf) {
                Ok(v) => messages.push(v),
                Err(e) => panic!(
                    "invalid JSON from server: {e}: {}",
                    String::from_utf8_lossy(&buf)
                ),
            }
        }
    });

    Proc {
        child,
        writer: stdin,
        reader_thread: Some(reader_thread),
    }
}

fn send(proc: &mut Proc, value: &serde_json::Value) {
    let body = serde_json::to_string(value).unwrap();
    write!(
        proc.writer,
        "Content-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write frame");
    proc.writer.flush().unwrap();
}

fn send_notification(proc: &mut Proc, method: &str, params: serde_json::Value) {
    send(
        proc,
        &serde_json::json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    );
}

fn send_request(proc: &mut Proc, id: i32, method: &str, params: serde_json::Value) {
    send(
        proc,
        &serde_json::json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    );
}

fn method_of(msg: &serde_json::Value) -> Option<&str> {
    msg.get("method").and_then(|m| m.as_str())
}

#[test]
fn stdio_end_to_end() {
    let tmp = TempDir::new("stdio");
    let main_uri = tmp.write(
        "main.rosetta",
        "namespace test\n\ntype Trade:\n    party Missing (1..1)\n",
    );
    let text = "namespace test\n\ntype Trade:\n    party Missing (1..1)\n";

    let mut proc = start();

    // initialize with positionEncodings offering utf-8.
    send_request(
        &mut proc,
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": common::path_to_uri(tmp.path()),
            "capabilities": {
                "general": { "positionEncodings": ["utf-8"] },
                "textDocument": {}
            },
        }),
    );
    send_notification(&mut proc, "initialized", serde_json::json!({}));

    // didOpen with an unknown type -> expect publishDiagnostics with E0101.
    send_notification(
        &mut proc,
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": main_uri,
                "languageId": "rosetta",
                "version": 1,
                "text": text,
            }
        }),
    );

    // shutdown + exit.
    send_request(&mut proc, 2, "shutdown", serde_json::Value::Null);
    send_notification(&mut proc, "exit", serde_json::Value::Null);

    // Drain everything the server said.
    let messages = proc.reader_thread.take().unwrap().join().unwrap();
    let status = proc.child.wait().expect("wait for server exit");
    assert!(status.success(), "server exit status: {status:?}");

    let initialize_result = messages
        .iter()
        .find(|m| m.get("id") == Some(&serde_json::json!(1)) && m.get("result").is_some())
        .expect("initialize response");
    let position_encoding = initialize_result["result"]["capabilities"]["positionEncoding"]
        .as_str()
        .expect("positionEncoding");
    assert_eq!(position_encoding, "utf-8", "server must negotiate utf-8");
    assert_eq!(
        initialize_result["result"]["serverInfo"]["name"], "sigil-lsp",
        "serverInfo name"
    );

    let publish = messages
        .iter()
        .find(|m| method_of(m) == Some("textDocument/publishDiagnostics"))
        .expect("publishDiagnostics notification");
    assert_eq!(publish["params"]["uri"], serde_json::json!(main_uri));
    let diagnostics = publish["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "E0101" && d["range"]["start"]["line"] == 3),
        "expected E0101 at line 3, got {diagnostics:?}"
    );

    let shutdown_result = messages
        .iter()
        .find(|m| m.get("id") == Some(&serde_json::json!(2)))
        .expect("shutdown response");
    assert!(
        shutdown_result.get("result").is_some(),
        "shutdown must succeed: {shutdown_result}"
    );
}
