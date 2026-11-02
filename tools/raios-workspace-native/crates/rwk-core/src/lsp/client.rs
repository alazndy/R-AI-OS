//! A language server child process and the conversation with it: requests that wait for their
//! answer, notifications, and the server's own diagnostics pushed back as events.
//!
//! Everything is threads and channels (no async runtime). One reader thread owns the server's
//! output; it routes responses to whoever waits for them and answers the requests a server
//! sends to its client, so a server never blocks on us.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::framing::{read_message, write_message};
use super::servers::ServerSpec;
use super::uri::path_to_uri;
use crate::error::{AppError, ErrorCode, Result};

/// How long `initialize` may take.
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// Most diagnostics kept for one file; a server cannot flood the panel.
pub const MAX_DIAGNOSTICS_PER_FILE: usize = 2000;
/// How long a server gets to exit after `shutdown` before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(2);

type Reply = Result<Value>;
type Pending = Arc<Mutex<HashMap<i64, Sender<Reply>>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a server tells the client without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    /// The full set of diagnostics for one document (an empty list clears it).
    Diagnostics {
        /// The document's URI as the server spelled it.
        uri: String,
        /// The document version the diagnostics belong to, when the server says.
        version: Option<i32>,
        /// The diagnostics, at most [`MAX_DIAGNOSTICS_PER_FILE`].
        diagnostics: Vec<lsp_types::Diagnostic>,
    },
    /// The server's output ended: it exited or crashed.
    Exited,
}

/// A running language server.
pub struct LspClient {
    stdin: Arc<Mutex<ChildStdin>>,
    child: Mutex<Child>,
    pending: Pending,
    next_id: AtomicI64,
    capabilities: Value,
}

fn ended() -> AppError {
    AppError::new(ErrorCode::Io, "the language server ended")
}

fn server_error(error: &Value) -> AppError {
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the language server refused the request");
    AppError::new(ErrorCode::Daemon, message.to_owned())
}

impl LspClient {
    /// Starts the server `spec` found at `program` in `root` and completes the handshake.
    pub fn start(
        spec: &ServerSpec,
        program: &Path,
        root: &Path,
    ) -> Result<(Self, Receiver<ServerEvent>)> {
        let mut command = Command::new(program);
        command.args(spec.args);
        Self::start_command(command, root)
    }

    /// Starts `command` as a server and completes the handshake. The seam tests use; the
    /// catalog path is [`Self::start`].
    pub fn start_command(
        mut command: Command,
        root: &Path,
    ) -> Result<(Self, Receiver<ServerEvent>)> {
        command
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or_else(ended)?;
        let stdout = child.stdout.take().ok_or_else(ended)?;
        let stdin = Arc::new(Mutex::new(stdin));
        let pending: Pending = Arc::default();
        let (events, receiver) = mpsc::channel();
        {
            let (stdin, pending) = (Arc::clone(&stdin), Arc::clone(&pending));
            std::thread::Builder::new()
                .name("lsp-reader".into())
                .spawn(move || reader_loop(BufReader::new(stdout), &stdin, &pending, &events))?;
        }
        let mut client = Self {
            stdin,
            child: Mutex::new(child),
            pending,
            next_id: AtomicI64::new(1),
            capabilities: Value::Null,
        };
        client.initialize(root)?;
        Ok((client, receiver))
    }

    fn initialize(&mut self, root: &Path) -> Result<()> {
        let root_uri = path_to_uri(root, "");
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let result = self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {"name": "raios-workspace-native"},
                "rootUri": root_uri,
                "workspaceFolders": [{"uri": root_uri, "name": name}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-16"]},
                    "workspace": {"configuration": true},
                    "textDocument": {
                        "synchronization": {"didSave": false},
                        "publishDiagnostics": {"relatedInformation": false},
                        "hover": {"contentFormat": ["plaintext", "markdown"]},
                        "completion": {"completionItem": {"snippetSupport": false}},
                        "definition": {"linkSupport": true},
                        "signatureHelp": {"signatureInformation": {
                            "parameterInformation": {"labelOffsetSupport": true},
                            "activeParameterSupport": true
                        }}
                    }
                }
            }),
            INITIALIZE_TIMEOUT,
        )?;
        self.capabilities = result.get("capabilities").cloned().unwrap_or(Value::Null);
        self.notify("initialized", json!({}))
    }

    /// What the server said it can do.
    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    fn send(&self, message: &Value) -> Result<()> {
        write_message(&mut *lock(&self.stdin), message)
    }

    /// Sends a request and waits for its answer. On a timeout the request is cancelled.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        lock(&self.pending).insert(id, reply);
        let sent =
            self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        if let Err(error) = sent {
            lock(&self.pending).remove(&id);
            return Err(error);
        }
        match answer.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id);
                let _ = self.notify("$/cancelRequest", json!({"id": id}));
                Err(AppError::new(
                    ErrorCode::Io,
                    format!("the language server did not answer {method} in time"),
                ))
            }
            Err(RecvTimeoutError::Disconnected) => Err(ended()),
        }
    }

    /// Sends a notification.
    pub fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Tells the server a document is open with this text.
    pub fn did_open(&self, uri: &str, language: &str, version: i32, text: &str) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": language, "version": version, "text": text}}),
        )
    }

    /// Tells the server the whole text of an open document changed.
    pub fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<()> {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
        )
    }

    /// Tells the server a document was closed.
    pub fn did_close(&self, uri: &str) -> Result<()> {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
    }

    /// Asks the server to stop, then ends it if it does not leave within a short grace.
    pub fn shutdown(&self) {
        let _ = self.request("shutdown", Value::Null, EXIT_GRACE);
        let _ = self.notify("exit", Value::Null);
        let deadline = Instant::now() + EXIT_GRACE;
        loop {
            let done = lock(&self.child).try_wait().ok().flatten().is_some();
            if done {
                return;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.kill();
    }

    fn kill(&self) {
        let mut child = lock(&self.child);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // A client dropped without `shutdown` must not leave a server running.
        self.kill();
    }
}

fn fail_all(pending: &Pending) {
    for (_, reply) in lock(pending).drain() {
        let _ = reply.send(Err(ended()));
    }
}

fn reader_loop(
    mut reader: BufReader<std::process::ChildStdout>,
    stdin: &Arc<Mutex<ChildStdin>>,
    pending: &Pending,
    events: &Sender<ServerEvent>,
) {
    while let Ok(Some(message)) = read_message(&mut reader) {
        route(&message, stdin, pending, events);
    }
    fail_all(pending);
    let _ = events.send(ServerEvent::Exited);
}

fn route(
    message: &Value,
    stdin: &Arc<Mutex<ChildStdin>>,
    pending: &Pending,
    events: &Sender<ServerEvent>,
) {
    let method = message.get("method").and_then(Value::as_str);
    let id = message.get("id");
    match (method, id) {
        // A request from the server: answer so it never waits on us.
        (Some(method), Some(id)) => {
            let result = match method {
                "workspace/configuration" => {
                    let count = message
                        .pointer("/params/items")
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; count.min(64)])
                }
                _ => Value::Null,
            };
            let _ = write_message(
                &mut *lock(stdin),
                &json!({"jsonrpc": "2.0", "id": id, "result": result}),
            );
        }
        (Some("textDocument/publishDiagnostics"), None) => {
            if let Some(params) = message.get("params") {
                if let Ok(params) =
                    serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params.clone())
                {
                    let mut diagnostics = params.diagnostics;
                    diagnostics.truncate(MAX_DIAGNOSTICS_PER_FILE);
                    let _ = events.send(ServerEvent::Diagnostics {
                        uri: params.uri.to_string(),
                        version: params.version,
                        diagnostics,
                    });
                }
            }
        }
        (Some(_), None) => {}
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            let Some(reply) = lock(pending).remove(&id) else {
                return;
            };
            let _ = reply.send(match message.get("error") {
                Some(error) => Err(server_error(error)),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            });
        }
        (None, None) => {}
    }
}
