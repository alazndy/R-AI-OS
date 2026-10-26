//! The language server manager against a fake server (this test binary started again with
//! `RWK_FAKE_LSP_CHILD` set). `harness = false` keeps the harness from printing to the
//! server's output.

use std::io::{BufReader, Write};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use rwk_core::lsp::framing::{read_message, write_message};
use rwk_core::lsp::{LanguageServers, Launcher, LspClient, ManagerEvent, ServerStatus};
use serde_json::{json, Value};

/// The server side. With `RWK_FAKE_LSP_DIE` it exits as soon as a document is opened.
fn serve() {
    let die = std::env::var_os("RWK_FAKE_LSP_DIE").is_some();
    let mut reader = BufReader::new(std::io::stdin().lock());
    let mut out = std::io::stdout();
    let mut say = |value: Value| {
        write_message(&mut out, &value).unwrap();
        out.flush().unwrap();
    };
    while let Ok(Some(message)) = read_message(&mut reader) {
        let id = message.get("id").cloned();
        match (message["method"].as_str(), id) {
            (Some("initialize"), Some(id)) => {
                say(json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {}}}));
            }
            (Some("textDocument/didOpen"), None) => {
                if die {
                    std::process::exit(3);
                }
                let uri = message["params"]["textDocument"]["uri"].clone();
                say(
                    json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": uri,
                    "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                                     "message": "boom"}]}}),
                );
            }
            (Some("shutdown"), Some(id)) => {
                say(json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            (Some("exit"), None) => std::process::exit(0),
            _ => {}
        }
    }
}

fn fake_command(die: bool) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.env("RWK_FAKE_LSP_CHILD", "1");
    if die {
        command.env("RWK_FAKE_LSP_DIE", "1");
    }
    command
}

/// A launcher whose first `dying` launches crash on the first document.
fn launcher(dying: usize, launches: Arc<AtomicUsize>) -> Launcher {
    Arc::new(move |_spec, root: &Path| {
        let number = launches.fetch_add(1, Ordering::SeqCst);
        LspClient::start_command(fake_command(number < dying), root)
    })
}

fn next(events: &Receiver<ManagerEvent>) -> ManagerEvent {
    events
        .recv_timeout(Duration::from_secs(15))
        .expect("a manager event")
}

fn next_diagnostics(events: &Receiver<ManagerEvent>) -> (String, usize) {
    loop {
        if let ManagerEvent::Diagnostics { path, diagnostics } = next(events) {
            return (path, diagnostics.len());
        }
    }
}

fn next_status(events: &Receiver<ManagerEvent>) -> ServerStatus {
    loop {
        if let ManagerEvent::Status { status, .. } = next(events) {
            if status != ServerStatus::Starting {
                return status;
            }
        }
    }
}

#[cfg(unix)]
fn a_server_starts_for_the_first_file_and_its_diagnostics_use_project_paths() {
    let dir = tempfile::tempdir().unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let (servers, events) =
        LanguageServers::new(dir.path().to_owned(), launcher(0, launches.clone()));

    servers.open("README.md", "# no server for this");
    servers.open("src/main.rs", "fn main() {}");
    assert_eq!(next_status(&events), ServerStatus::Ready);
    assert_eq!(next_diagnostics(&events), ("src/main.rs".to_owned(), 1));

    // A second file of the language reuses the server.
    servers.open("src/lib.rs", "");
    assert_eq!(next_diagnostics(&events), ("src/lib.rs".to_owned(), 1));
    assert_eq!(launches.load(Ordering::SeqCst), 1);

    // Closing clears the file's diagnostics.
    servers.close("src/lib.rs");
    assert_eq!(next_diagnostics(&events), ("src/lib.rs".to_owned(), 0));
    servers.shutdown();
}

fn a_missing_server_is_a_quiet_state_and_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let launches = Arc::new(AtomicUsize::new(0));
    let counted = launches.clone();
    let launcher: Launcher = Arc::new(move |_, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Err(rwk_core::error::AppError::new(
            rwk_core::error::ErrorCode::NotFound,
            "not installed",
        ))
    });
    let (servers, events) = LanguageServers::new(dir.path().to_owned(), launcher);
    servers.open("a.rs", "");
    assert!(matches!(next_status(&events), ServerStatus::Unavailable(_)));
    servers.open("b.rs", "");
    servers.shutdown();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
fn a_crashed_server_is_restarted_once_and_then_given_up() {
    let dir = tempfile::tempdir().unwrap();

    // Crashes once, then works: diagnostics arrive from the second server.
    let launches = Arc::new(AtomicUsize::new(0));
    let (servers, events) =
        LanguageServers::new(dir.path().to_owned(), launcher(1, launches.clone()));
    servers.open("a.rs", "");
    assert_eq!(next_diagnostics(&events), ("a.rs".to_owned(), 1));
    assert_eq!(launches.load(Ordering::SeqCst), 2);
    servers.shutdown();

    // Crashes every time: reported as crashed, not restarted forever.
    let launches = Arc::new(AtomicUsize::new(0));
    let (servers, events) =
        LanguageServers::new(dir.path().to_owned(), launcher(9, launches.clone()));
    servers.open("a.rs", "");
    loop {
        if next_status(&events) == ServerStatus::Crashed {
            break;
        }
    }
    assert_eq!(launches.load(Ordering::SeqCst), 2);
    servers.shutdown();
}

fn main() {
    if std::env::var_os("RWK_FAKE_LSP_CHILD").is_some() {
        serve();
        return;
    }
    #[allow(unused_mut)]
    let mut tests: Vec<(&str, fn())> = vec![(
        "a_missing_server_is_a_quiet_state_and_is_not_retried",
        a_missing_server_is_a_quiet_state_and_is_not_retried,
    )];
    #[cfg(unix)]
    {
        tests.push((
            "a_server_starts_for_the_first_file_and_its_diagnostics_use_project_paths",
            a_server_starts_for_the_first_file_and_its_diagnostics_use_project_paths,
        ));
        tests.push((
            "a_crashed_server_is_restarted_once_and_then_given_up",
            a_crashed_server_is_restarted_once_and_then_given_up,
        ));
    }
    for (name, test) in tests {
        print!("test {name} ... ");
        std::io::stdout().flush().unwrap();
        test();
        println!("ok");
    }
}
