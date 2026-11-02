//! The language server client against a fake server. The fake is this test binary started again
//! with `RWK_FAKE_LSP_CHILD` set, so no real language server is needed. The test target has its
//! own `main` (`harness = false`) because the test harness would print to the server's output.

use std::io::{BufReader, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rwk_core::lsp::framing::{read_message, write_message};
use rwk_core::lsp::{LspClient, ServerEvent};
use serde_json::{json, Value};

/// The server side. Runs only in the child process.
fn serve() {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    let mut say = |value: Value| {
        write_message(&mut out, &value).unwrap();
        out.flush().unwrap();
    };
    while let Ok(Some(message)) = read_message(&mut reader) {
        let id = message.get("id").cloned();
        match (message["method"].as_str(), id) {
            (Some("initialize"), Some(id)) => {
                say(
                    json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {"hoverProvider": true}}}),
                );
                // A request from the server: the client must answer it.
                say(
                    json!({"jsonrpc": "2.0", "id": 900, "method": "workspace/configuration", "params": {"items": [{}, {}]}}),
                );
            }
            (Some("echo"), Some(id)) => {
                say(json!({"jsonrpc": "2.0", "id": id, "result": message["params"]}));
            }
            (Some("fail"), Some(id)) => {
                say(
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no such thing"}}),
                );
            }
            (Some("textDocument/didOpen"), None) => {
                let uri = message["params"]["textDocument"]["uri"].clone();
                say(
                    json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": uri, "version": 1,
                    "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                                     "severity": 1, "message": "boom"}]}}),
                );
            }
            (Some("die"), None) => std::process::exit(3),
            (Some("shutdown"), Some(id)) => {
                say(json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            (Some("exit"), None) => std::process::exit(0),
            // `slow` and anything unknown: no answer.
            (None, Some(id)) if id == 900 => {
                let count = message["result"].as_array().map_or(0, Vec::len);
                say(
                    json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": "file:///cfg", "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                    "message": format!("config answered with {count}")}]}}),
                );
            }
            _ => {}
        }
    }
}

fn start(root: &Path) -> (LspClient, Receiver<ServerEvent>) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "fake_server_entry",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("RWK_FAKE_LSP_CHILD", "1");
    LspClient::start_command(command, root).expect("the fake server starts")
}

fn next_event(events: &Receiver<ServerEvent>) -> ServerEvent {
    events
        .recv_timeout(Duration::from_secs(10))
        .expect("an event")
}

#[cfg(unix)]
fn the_handshake_requests_and_server_requests_work() {
    let dir = tempfile::tempdir().unwrap();
    let (client, events) = start(dir.path());
    assert_eq!(client.capabilities()["hoverProvider"], true);

    // The server's own request was answered with one null per item.
    let ServerEvent::Diagnostics {
        uri, diagnostics, ..
    } = next_event(&events)
    else {
        panic!("diagnostics first");
    };
    assert_eq!(uri, "file:///cfg");
    assert_eq!(diagnostics[0].message, "config answered with 2");

    let echoed = client
        .request("echo", json!({"a": 1}), Duration::from_secs(5))
        .unwrap();
    assert_eq!(echoed["a"], 1);
    let error = client
        .request("fail", Value::Null, Duration::from_secs(5))
        .unwrap_err();
    assert!(error.message.contains("no such thing"), "{}", error.message);
    client.shutdown();
}

#[cfg(unix)]
fn opening_a_document_brings_its_diagnostics_back_and_a_silent_request_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let (client, events) = start(dir.path());
    let _config = next_event(&events);

    client
        .did_open("file:///work/a.rs", "rust", 1, "fn main() {}")
        .unwrap();
    let ServerEvent::Diagnostics {
        uri,
        version,
        diagnostics,
    } = next_event(&events)
    else {
        panic!("diagnostics");
    };
    assert_eq!(uri, "file:///work/a.rs");
    assert_eq!(version, Some(1));
    assert_eq!(diagnostics[0].message, "boom");

    let started = std::time::Instant::now();
    let error = client
        .request("slow", Value::Null, Duration::from_millis(300))
        .unwrap_err();
    assert!(error.message.contains("in time"), "{}", error.message);
    assert!(started.elapsed() < Duration::from_secs(5));
    // The client still works after a timeout.
    assert!(client
        .request("echo", json!(1), Duration::from_secs(5))
        .is_ok());
    client.shutdown();
}

#[cfg(unix)]
fn a_crashed_server_fails_waiting_requests_and_reports_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (client, events) = start(dir.path());
    let _config = next_event(&events);

    client.notify("die", Value::Null).unwrap();
    loop {
        if next_event(&events) == ServerEvent::Exited {
            break;
        }
    }
    let error = client
        .request("echo", Value::Null, Duration::from_secs(5))
        .unwrap_err();
    assert!(!error.message.is_empty());
}

#[cfg(unix)]
fn a_program_that_is_not_a_language_server_fails_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::new("true");
    command.arg("--stdio");
    let started = std::time::Instant::now();
    // `true` exits at once without answering initialize: the handshake must not hang for 30 s.
    let result = LspClient::start_command(command, dir.path());
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(10));
}

fn main() {
    if std::env::var_os("RWK_FAKE_LSP_CHILD").is_some() {
        serve();
        return;
    }
    #[cfg(unix)]
    {
        let tests: [(&str, fn()); 4] = [
            (
                "the_handshake_requests_and_server_requests_work",
                the_handshake_requests_and_server_requests_work,
            ),
            (
                "opening_a_document_brings_its_diagnostics_back_and_a_silent_request_times_out",
                opening_a_document_brings_its_diagnostics_back_and_a_silent_request_times_out,
            ),
            (
                "a_crashed_server_fails_waiting_requests_and_reports_exit",
                a_crashed_server_fails_waiting_requests_and_reports_exit,
            ),
            (
                "a_program_that_is_not_a_language_server_fails_to_start",
                a_program_that_is_not_a_language_server_fails_to_start,
            ),
        ];
        let mut failed = 0;
        for (name, test) in tests {
            match std::panic::catch_unwind(test) {
                Ok(()) => println!("test {name} ... ok"),
                Err(_) => {
                    println!("test {name} ... FAILED");
                    failed += 1;
                }
            }
        }
        if failed > 0 {
            std::process::exit(1);
        }
    }
}
