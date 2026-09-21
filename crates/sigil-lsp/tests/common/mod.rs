//! Shared test harness: a headless LSP client driving the sigil-lsp server
//! through an in-memory [`lsp_server::Connection`] pair.
//!
//! The client keeps a text mirror of every open document so tests can
//! compare server state against what the client believes. All waits poll the
//! crossbeam channel with a timeout — there are no sleeps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{Notification as _, PublishDiagnostics};
use lsp_types::request::{Initialize, Request as _, Shutdown};
use lsp_types::{
    ClientCapabilities, Diagnostic, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, PositionEncodingKind,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri, VersionedTextDocumentIdentifier, WorkspaceFolder,
};

use sigil_lsp::{run_server, World};

pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A self-cleaning temporary directory for workspace roots.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sigil-lsp-test-{}-{}-{}",
            tag,
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Write a file into the workspace and return its `file://` URI.
    pub fn write(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, text).expect("write file");
        path_to_uri(&path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn path_to_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// Build an LSP `Uri` from a string, bypassing constructor-API differences
/// between lsp-types versions.
pub fn uri(s: &str) -> Uri {
    serde_json::from_value(serde_json::Value::String(s.to_string())).expect("valid uri")
}

/// A headless LSP client connected to a live `sigil-lsp` server loop.
/// (Dead-code allowances: this harness is compiled once per test target and
/// not every target exercises every method.)
#[allow(dead_code)]
pub struct TestClient {
    conn: Connection,
    next_id: i64,
    pub server_capabilities: InitializeResult,
    /// Last publishDiagnostics per uri.
    pub diagnostics: HashMap<String, PublishDiagnosticsParams>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl TestClient {
    /// Spawn the server over an in-memory connection and perform the full
    /// initialize/initialized handshake.
    ///
    /// `client_caps` is a JSON fragment merged into `capabilities`, e.g. to
    /// control `general.positionEncodings`.
    pub fn spawn(root: &Path, client_caps: serde_json::Value) -> Self {
        let (client_conn, server_conn) = Connection::memory();
        let world = World::new(Some(root.to_path_buf()));
        let handle = std::thread::spawn(move || {
            run_server(server_conn, world).expect("server loop failed");
        });

        let capabilities = merge(
            serde_json::to_value(ClientCapabilities::default()).unwrap(),
            client_caps,
        );
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: None,
            root_path: None,
            root_uri: Some(uri(&path_to_uri(root))),
            initialization_options: None,
            capabilities: serde_json::from_value(capabilities.clone()).unwrap(),
            trace: None,
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: uri(&path_to_uri(root)),
                name: "test".into(),
            }]),
            client_info: None,
            locale: None,
            work_done_progress_params: Default::default(),
        };

        let mut client = TestClient {
            conn: client_conn,
            next_id: 1,
            server_capabilities: InitializeResult::default(),
            diagnostics: HashMap::new(),
            handle: Some(handle),
        };
        let result: InitializeResult = client.request_typed::<Initialize>(params);
        client.server_capabilities = result;
        // Complete the handshake: `initialized` must follow the initialize
        // response before anything else is sent.
        client
            .conn
            .sender
            .send(
                lsp_server::Notification::new(
                    lsp_types::notification::Initialized::METHOD.to_string(),
                    serde_json::to_value(lsp_types::InitializedParams {}).unwrap(),
                )
                .into(),
            )
            .unwrap();
        client
    }

    pub fn spawn_utf8(root: &Path) -> Self {
        Self::spawn(
            root,
            serde_json::json!({
                "general": { "positionEncodings": [
                    PositionEncodingKind::UTF8, PositionEncodingKind::UTF16
                ]}
            }),
        )
    }

    /// No `general.positionEncodings`: the server must fall back to UTF-16.
    pub fn spawn_utf16(root: &Path) -> Self {
        Self::spawn(root, serde_json::json!({}))
    }

    // ---- lifecycle ------------------------------------------------------

    pub fn shutdown(mut self) {
        let id = self.next_request_id();
        self.conn
            .sender
            .send(
                lsp_server::Request::new(
                    id.clone(),
                    Shutdown::METHOD.to_string(),
                    serde_json::Value::Null,
                )
                .into(),
            )
            .unwrap();
        let resp = self.pump_until_response(&id);
        assert!(
            resp.response_result.is_ok(),
            "shutdown failed: {:?}",
            resp.response_result
        );
        self.conn
            .sender
            .send(
                lsp_server::Notification::new(
                    lsp_types::notification::Exit::METHOD.to_string(),
                    serde_json::Value::Null,
                )
                .into(),
            )
            .unwrap();
        self.handle.take().unwrap().join().expect("server panicked");
    }

    // ---- text sync ------------------------------------------------------

    pub fn open(&mut self, uri_str: &str, text: &str, version: i32) {
        self.conn
            .sender
            .send(
                lsp_server::Notification::new(
                    lsp_types::notification::DidOpenTextDocument::METHOD.to_string(),
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: lsp_types::TextDocumentItem {
                            uri: uri(uri_str),
                            language_id: "rosetta".into(),
                            version,
                            text: text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .into(),
            )
            .unwrap();
    }

    /// Send an incremental didChange. Ranges are interpreted against the
    /// text *before* the first edit in `edits` (each subsequent edit within
    /// the batch applies to the text after the previous one, matching the
    /// server).
    pub fn change(&mut self, uri_str: &str, version: i32, edits: Vec<(lsp_types::Range, String)>) {
        let changes: Vec<TextDocumentContentChangeEvent> = edits
            .into_iter()
            .map(|(range, text)| TextDocumentContentChangeEvent {
                range: Some(range),
                range_length: None,
                text,
            })
            .collect();
        self.send_change(uri_str, version, changes);
    }

    pub fn change_full(&mut self, uri_str: &str, version: i32, text: &str) {
        self.send_change(
            uri_str,
            version,
            vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_string(),
            }],
        );
    }

    fn send_change(
        &mut self,
        uri_str: &str,
        version: i32,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) {
        self.conn
            .sender
            .send(
                lsp_server::Notification::new(
                    lsp_types::notification::DidChangeTextDocument::METHOD.to_string(),
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: uri(uri_str),
                            version,
                        },
                        content_changes: changes,
                    })
                    .unwrap(),
                )
                .into(),
            )
            .unwrap();
    }

    pub fn close(&mut self, uri_str: &str) {
        self.conn
            .sender
            .send(
                lsp_server::Notification::new(
                    lsp_types::notification::DidCloseTextDocument::METHOD.to_string(),
                    serde_json::to_value(DidCloseTextDocumentParams {
                        text_document: lsp_types::TextDocumentIdentifier { uri: uri(uri_str) },
                    })
                    .unwrap(),
                )
                .into(),
            )
            .unwrap();
    }

    // ---- requests -------------------------------------------------------

    fn next_request_id(&mut self) -> RequestId {
        let id = RequestId::from(self.next_id as i32);
        self.next_id += 1;
        id
    }

    /// Typed request helper for standard LSP requests.
    pub fn request<R>(&mut self, params: R::Params) -> R::Result
    where
        R: lsp_types::request::Request,
        R::Params: serde::Serialize,
        R::Result: serde::de::DeserializeOwned,
    {
        self.request_typed::<R>(params)
    }

    fn request_typed<R>(&mut self, params: R::Params) -> R::Result
    where
        R: lsp_types::request::Request,
        R::Params: serde::Serialize,
        R::Result: serde::de::DeserializeOwned,
    {
        let id = self.next_request_id();
        self.conn
            .sender
            .send(
                Request::new(
                    id.clone(),
                    R::METHOD.to_string(),
                    serde_json::to_value(params).unwrap(),
                )
                .into(),
            )
            .unwrap();
        let resp = self.pump_until_response(&id);
        let result = resp
            .response_result
            .unwrap_or_else(|e| panic!("{} failed: {e:?}", R::METHOD));
        let result = if result.is_null() {
            serde_json::Value::Null
        } else {
            result
        };
        serde_json::from_value(result).expect("deserialize response")
    }

    /// The server's current text for `uri` (via the custom `sigil/textDocument`
    /// request). Also serves as a synchronization barrier: the server is
    /// FIFO, so by the time the response arrives every effect of earlier
    /// notifications — publishDiagnostics included — is already queued and
    /// gets absorbed here.
    pub fn server_text(&mut self, uri_str: &str) -> Option<String> {
        self.request::<sigil_lsp::DocumentTextRequest>(sigil_lsp::DocumentTextParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(uri_str) },
        })
    }

    /// Deterministic barrier: pump until the server answers, absorbing all
    /// diagnostics triggered by preceding notifications. After `sync()`,
    /// `latest_diagnostics` reflects the server's final state.
    pub fn sync(&mut self, any_known_uri: &str) {
        let _ = self.server_text(any_known_uri);
    }

    // ---- notifications / diagnostics -----------------------------------

    /// Pump messages until a publishDiagnostics for `uri_str` with
    /// `version >= min_version` arrives. Deterministic: versions are bumped
    /// by open/change.
    pub fn expect_diagnostics(&mut self, uri_str: &str, min_version: i32) -> Vec<Diagnostic> {
        let deadline = std::time::Instant::now() + TIMEOUT;
        loop {
            if std::time::Instant::now() > deadline {
                panic!("timed out waiting for diagnostics for {uri_str} >= v{min_version}");
            }
            self.pump_one(deadline);
            if let Some(publish) = self.diagnostics.get(uri_str) {
                if publish.version.unwrap_or(0) >= min_version {
                    return publish.diagnostics.clone();
                }
            }
        }
    }

    pub fn latest_diagnostics(&self, uri_str: &str) -> Vec<Diagnostic> {
        self.diagnostics
            .get(uri_str)
            .map(|p| p.diagnostics.clone())
            .unwrap_or_default()
    }

    /// Wait for one more message (diagnostics or otherwise) with a timeout.
    pub fn wait_for_notification(&mut self) -> Option<Notification> {
        let deadline = std::time::Instant::now() + TIMEOUT;
        self.pump_one(deadline);
        None
    }

    // ---- low-level pumping ----------------------------------------------

    fn pump_until_response(&mut self, id: &RequestId) -> Response {
        let deadline = std::time::Instant::now() + TIMEOUT;
        loop {
            if std::time::Instant::now() > deadline {
                panic!("timed out waiting for response {id:?}");
            }
            match self.conn.receiver.recv_timeout(TIMEOUT) {
                Ok(Message::Response(resp)) if &resp.id == id => return resp,
                Ok(msg) => self.absorb(msg),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    panic!("timed out waiting for response {id:?}")
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    panic!("server channel disconnected while waiting for {id:?}")
                }
            }
        }
    }

    /// Read at most one message, buffering it. Returns false on timeout.
    fn pump_one(&mut self, deadline: std::time::Instant) -> bool {
        let timeout = deadline.saturating_duration_since(std::time::Instant::now());
        match self.conn.receiver.recv_timeout(timeout) {
            Ok(msg) => {
                self.absorb(msg);
                true
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => false,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => false,
        }
    }

    fn absorb(&mut self, msg: Message) {
        match msg {
            Message::Notification(n) if n.method == PublishDiagnostics::METHOD => {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(n.params).expect("valid publishDiagnostics");
                self.diagnostics.insert(params.uri.to_string(), params);
            }
            Message::Notification(_) => {}
            Message::Request(req) => {
                // The server must not issue requests we don't understand.
                panic!("unexpected server->client request: {}", req.method);
            }
            Message::Response(resp) => {
                panic!("unexpected response to client-initiated id {:?}", resp.id);
            }
        }
    }
}

fn merge(base: serde_json::Value, patch: serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(mut b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                let merged = match b.remove(&k) {
                    Some(existing) => merge(existing, v),
                    None => v,
                };
                b.insert(k, merged);
            }
            serde_json::Value::Object(b)
        }
        (b, p) => {
            let _ = b;
            p
        }
    }
}

// Allowances: this module is compiled once per test target; not every
// target uses every helper.
#[allow(dead_code)]
pub fn code_of(d: &Diagnostic) -> Option<&str> {
    match &d.code {
        Some(lsp_types::NumberOrString::String(s)) => Some(s),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn assert_codes(diags: &[Diagnostic], expected: &[(&str, u32, u32)]) {
    let got: Vec<(String, u32, u32)> = diags
        .iter()
        .map(|d| {
            (
                code_of(d).expect("diagnostic code").to_string(),
                d.range.start.line,
                d.range.start.character,
            )
        })
        .collect();
    let want: Vec<(String, u32, u32)> = expected
        .iter()
        .map(|(c, l, ch)| (c.to_string(), *l, *ch))
        .collect();
    assert_eq!(got, want, "unexpected diagnostics: {diags:?}");
}

#[allow(dead_code)]
pub fn assert_incremental_sync(caps: &InitializeResult) {
    let sync = caps
        .capabilities
        .text_document_sync
        .as_ref()
        .expect("textDocumentSync capability");
    match sync {
        TextDocumentSyncCapability::Options(opts) => {
            assert_eq!(opts.open_close, Some(true));
            assert_eq!(opts.change, Some(TextDocumentSyncKind::INCREMENTAL));
        }
        other => panic!("unexpected textDocumentSync: {other:?}"),
    }
}
