//! `DaemonClient` against a scripted local HTTP server that captures every request it receives.
//!
//! Capturing the request matters: a mock that answers 200 to anything cannot notice a missing
//! `Authorization` header or a wrong body.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use raios_contracts::{GovernSnapshot, WorkSnapshot};
use rwk_core::control::daemon::{DaemonClient, TokenSource};
use rwk_core::control::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
};
use rwk_core::error::{AppError, ErrorCode, Result};
use rwk_core::sessions::SessionKind;
use serde_json::{json, Value};

struct Reply {
    status: u16,
    body: String,
}

fn ok(body: Value) -> Reply {
    Reply {
        status: 200,
        body: body.to_string(),
    }
}

fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.to_owned(),
    }
}

/// Longest the scripted daemon waits for a connection. A client that never sends the expected
/// request must fail the test, not hang it.
const ACCEPT_DEADLINE: Duration = Duration::from_secs(5);

fn accept_within(listener: &TcpListener, deadline: Duration) -> TcpStream {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < deadline,
                    "the client sent no request within {deadline:?}"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    }
}

/// Serves exactly `replies.len()` connections, one reply each, and returns the captured
/// requests (request line, headers and body) once they have all been served.
fn spawn_daemon(replies: Vec<Reply>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in replies {
            let mut stream = accept_within(&listener, ACCEPT_DEADLINE);
            requests.push(read_request(&mut stream));
            let reason = match reply.status {
                200 => "OK",
                401 => "Unauthorized",
                403 => "Forbidden",
                500 => "Internal Server Error",
                _ => "Status",
            };
            let response = format!(
                "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.status,
                reply.body.len(),
                reply.body
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    (format!("http://{address}"), handle)
}

/// Reads one HTTP/1.1 request: the headers, then exactly `Content-Length` body bytes.
fn read_request(stream: &mut TcpStream) -> String {
    let mut raw = Vec::new();
    let mut byte = [0u8; 1];
    while !raw.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => return String::from_utf8_lossy(&raw).into_owned(),
            Ok(_) => raw.push(byte[0]),
        }
    }
    let headers = String::from_utf8_lossy(&raw).into_owned();
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 && stream.read_exact(&mut body).is_err() {
        body.clear();
    }
    format!("{headers}{}", String::from_utf8_lossy(&body))
}

fn json_body(request: &str) -> Value {
    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    serde_json::from_str(body).unwrap_or(Value::Null)
}

fn has_bearer(request: &str, token: &str) -> bool {
    request.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("authorization")
                && value.trim() == format!("Bearer {token}")
        })
    })
}

struct FixedToken(&'static str);

impl TokenSource for FixedToken {
    fn bearer_token(&self) -> Result<String> {
        Ok(self.0.to_owned())
    }
}

struct BrokenToken;

impl TokenSource for BrokenToken {
    fn bearer_token(&self) -> Result<String> {
        Err(AppError::new(
            ErrorCode::Unauthorized,
            "no session token file",
        ))
    }
}

fn client(base_url: &str) -> DaemonClient {
    DaemonClient::new(base_url, Box::new(FixedToken("test-token")))
}

/// A base URL nothing listens on, to prove a call never reached the network.
fn closed_port_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    url
}

fn problem(code: &str, message: &str) -> Value {
    json!({"status": "error", "problem": {"code": code, "message": message, "details": null, "retryable": false}})
}

#[test]
fn a_work_snapshot_query_is_posted_with_the_session_token() {
    let snapshot = serde_json::to_value(WorkSnapshot::default()).unwrap();
    let (url, server) = spawn_daemon(vec![ok(json!({"status": "ok", "work": snapshot}))]);

    let result = client(&url).work_snapshot().unwrap();

    let requests = server.join().unwrap();
    assert_eq!(result, WorkSnapshot::default());
    assert!(
        requests[0].starts_with("POST /api/v1/control/query "),
        "{}",
        requests[0]
    );
    assert!(has_bearer(&requests[0], "test-token"), "{}", requests[0]);
    assert_eq!(json_body(&requests[0])["query_type"], "GetWorkSnapshot");
}

#[test]
fn a_govern_snapshot_is_read_from_the_govern_field() {
    let snapshot = serde_json::to_value(GovernSnapshot::default()).unwrap();
    let (url, server) = spawn_daemon(vec![ok(json!({"status": "ok", "govern": snapshot}))]);

    let result = client(&url).govern_snapshot().unwrap();

    let requests = server.join().unwrap();
    assert_eq!(result, GovernSnapshot::default());
    assert_eq!(json_body(&requests[0])["query_type"], "GetGovernSnapshot");
}

#[test]
fn command_policy_maps_the_daemons_label_and_rejects_unknown_ones() {
    let (url, server) = spawn_daemon(vec![
        ok(
            json!({"status": "ok", "command_policy": {"command_name": "launch_agent", "action": "confirm"}}),
        ),
        ok(
            json!({"status": "ok", "command_policy": {"command_name": "launch_agent", "action": "permit"}}),
        ),
    ]);
    let daemon = client(&url);

    assert_eq!(
        daemon.command_policy("launch_agent").unwrap(),
        PolicyAction::Confirm
    );
    let unknown = daemon.command_policy("launch_agent").unwrap_err();

    let requests = server.join().unwrap();
    assert_eq!(unknown.code, ErrorCode::Daemon);
    let body = json_body(&requests[0]);
    assert_eq!(body["query_type"], "GetCommandPolicy");
    assert_eq!(body["payload"]["command_name"], "launch_agent");
}

#[test]
fn a_handoff_receipt_is_scoped_to_its_project_and_keeps_the_delivery_flag() {
    let (url, server) = spawn_daemon(vec![ok(json!({"status": "ok", "handoff_receipt": {
        "approval_id": "a-1", "project_path": "/work/app", "from_agent": "claude_kaira",
        "to_agent": "codex_kaira", "status": "approved", "resolved_by": "codex_kaira",
        "resolved_at": "2026-10-06 01:00:00", "delivered_by_target": true
    }}))]);

    let receipt = client(&url)
        .handoff_receipt("a-1", Path::new("/work/app"))
        .unwrap();

    let requests = server.join().unwrap();
    assert!(receipt.delivered_by_target);
    assert_eq!(receipt.status, "approved");
    let body = json_body(&requests[0]);
    assert_eq!(body["query_type"], "GetHandoffReceipt");
    assert_eq!(body["payload"]["approval_id"], "a-1");
    assert_eq!(body["payload"]["project_path"], "/work/app");
}

#[test]
fn creating_a_handoff_sends_agent_identities_and_returns_the_queued_receipt() {
    let (url, server) = spawn_daemon(vec![ok(json!({"status": "ok", "result": {
        "status": "queued", "approval_id": "a-9", "project_path": "/work/app"
    }}))]);

    let queued = client(&url)
        .create_handoff(&CreateHandoffRequest {
            project_root: PathBuf::from("/work/app"),
            from: SessionKind::Claude,
            to: SessionKind::Agy,
            summary: "Continue the migration".into(),
            idempotency_key: IdempotencyKey::new("h-1"),
        })
        .unwrap();

    let requests = server.join().unwrap();
    assert_eq!(queued.approval_id, "a-9");
    assert_eq!(queued.project_path, PathBuf::from("/work/app"));
    assert!(
        requests[0].starts_with("POST /api/v1/control/command "),
        "{}",
        requests[0]
    );
    assert!(has_bearer(&requests[0], "test-token"));
    let body = json_body(&requests[0]);
    assert_eq!(body["command_type"], "CreateHandoff");
    assert_eq!(body["payload"]["from_agent"], "claude_kaira");
    assert_eq!(body["payload"]["to_agent"], "antigravity_kaira");
    assert_eq!(body["payload"]["summary"], "Continue the migration");
    assert_eq!(body["payload"]["idempotency_key"], "h-1");
}

/// The exact JSON `raios-contracts` documents for `ValidateAgentLaunch`
/// (`crates/raios-contracts/tests/launch_contract.rs` holds the same literal), so a rename on
/// either side fails a test.
const VALIDATE_AGENT_LAUNCH_WIRE_JSON: &str = r#"{"command_type":"ValidateAgentLaunch","payload":{"agent_name":"opencode","project_path":"/work/app","prompt":"inspect the build","idempotency_key":"v-1"}}"#;

fn launch_request(agent: SessionKind, key: &str) -> LaunchAgentRequest {
    LaunchAgentRequest {
        project_root: PathBuf::from("/work/app"),
        agent,
        prompt: Some("inspect the build".into()),
        idempotency_key: IdempotencyKey::new(key),
    }
}

fn validated(agent_name: &str) -> Reply {
    ok(json!({"status": "ok", "result": {
        "agent_name": agent_name, "project_path": "/work/app", "status": "validated"
    }}))
}

#[test]
fn launching_an_agent_sends_validate_agent_launch_and_never_launch_agent() {
    // `Command::LaunchAgent` would make the daemon open an external terminal window; the
    // workspace runs agents in its own PTYs, so it must send only the validating command.
    let (url, server) = spawn_daemon(vec![validated("opencode")]);

    client(&url)
        .launch_agent(&launch_request(SessionKind::Opencode, "v-1"))
        .unwrap();

    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(
        request.starts_with("POST /api/v1/control/command "),
        "{request}"
    );
    assert!(has_bearer(request, "test-token"));
    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    assert_eq!(body, VALIDATE_AGENT_LAUNCH_WIRE_JSON);
    assert_ne!(json_body(request)["command_type"], "LaunchAgent");
}

#[test]
fn a_daemon_refusal_to_validate_is_returned_with_its_error_code() {
    let (url, server) = spawn_daemon(vec![
        ok(problem(
            "NOT_FOUND",
            "Agent launch requires a registered project path",
        )),
        ok(problem(
            "FORBIDDEN",
            "Security policy denies control command 'validate_agent_launch'",
        )),
    ]);
    let daemon = client(&url);

    let unregistered = daemon
        .launch_agent(&launch_request(SessionKind::Claude, "v-2"))
        .unwrap_err();
    let denied = daemon
        .launch_agent(&launch_request(SessionKind::Claude, "v-3"))
        .unwrap_err();

    server.join().unwrap();
    assert_eq!(unregistered.code, ErrorCode::NotFound);
    assert_eq!(denied.code, ErrorCode::Forbidden);
}

#[test]
fn a_daemon_without_the_command_fails_closed() {
    // An older daemon cannot decode the unknown `command_type` and answers HTTP 422.
    let (url, server) = spawn_daemon(vec![reply(422, "unknown variant `ValidateAgentLaunch`")]);

    let error = client(&url)
        .launch_agent(&launch_request(SessionKind::Codex, "v-4"))
        .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.code, ErrorCode::Daemon);
    assert!(error.message.contains("422"), "{}", error.message);
}

#[test]
fn any_result_other_than_validated_is_refused() {
    // `launching` is what `LaunchAgent` reports after it has opened a terminal. Treating it as
    // success would start a second copy of the agent in the workspace.
    let (url, server) = spawn_daemon(vec![
        ok(json!({"status": "ok", "result": {"status": "launching"}})),
        ok(json!({"status": "ok"})),
    ]);
    let daemon = client(&url);

    let launching = daemon
        .launch_agent(&launch_request(SessionKind::Claude, "v-5"))
        .unwrap_err();
    let missing = daemon
        .launch_agent(&launch_request(SessionKind::Claude, "v-6"))
        .unwrap_err();

    server.join().unwrap();
    assert_eq!(launching.code, ErrorCode::Daemon);
    assert!(
        launching.message.contains("launching"),
        "{}",
        launching.message
    );
    assert_eq!(missing.code, ErrorCode::Daemon);
}

#[test]
fn registering_a_project_reports_whether_it_was_already_registered() {
    let (url, server) = spawn_daemon(vec![
        ok(
            json!({"status": "ok", "result": {"status": "registered", "project_path": "/work/app"}}),
        ),
        ok(
            json!({"status": "ok", "result": {"status": "already_registered", "project_path": "/work/app"}}),
        ),
    ]);
    let daemon = client(&url);

    let first = daemon
        .register_project(Path::new("/work/app"), &IdempotencyKey::new("r-1"))
        .unwrap();
    let second = daemon
        .register_project(Path::new("/work/app"), &IdempotencyKey::new("r-2"))
        .unwrap();

    let requests = server.join().unwrap();
    assert!(!first.already_registered);
    assert!(second.already_registered);
    assert_eq!(first.project_path, PathBuf::from("/work/app"));
    assert_eq!(json_body(&requests[0])["command_type"], "RegisterProject");
}

#[test]
fn a_shell_session_is_refused_locally_without_contacting_the_daemon() {
    let daemon = client(&closed_port_url());

    let launch = daemon
        .launch_agent(&LaunchAgentRequest {
            project_root: PathBuf::from("/work/app"),
            agent: SessionKind::Shell,
            prompt: None,
            idempotency_key: IdempotencyKey::new("l-1"),
        })
        .unwrap_err();
    let handoff = daemon
        .create_handoff(&CreateHandoffRequest {
            project_root: PathBuf::from("/work/app"),
            from: SessionKind::Shell,
            to: SessionKind::Codex,
            summary: "x".into(),
            idempotency_key: IdempotencyKey::new("h-1"),
        })
        .unwrap_err();

    // `InvalidInput`, not `DaemonUnreachable`: the request was never attempted.
    assert_eq!(launch.code, ErrorCode::InvalidInput);
    assert_eq!(handoff.code, ErrorCode::InvalidInput);
}

#[test]
fn a_failed_token_lookup_stops_the_call_before_any_request() {
    let daemon = DaemonClient::new(closed_port_url(), Box::new(BrokenToken));

    let error = daemon.work_snapshot().unwrap_err();

    assert_eq!(error.code, ErrorCode::Unauthorized);
}

#[test]
fn daemon_problems_keep_their_meaning() {
    let (url, server) = spawn_daemon(vec![
        ok(problem("NOT_FOUND", "no such approval")),
        ok(problem("FORBIDDEN", "policy denies this command")),
        ok(problem("SOMETHING_NEW", "unmodelled")),
    ]);
    let daemon = client(&url);

    let not_found = daemon.handoff_receipt("a", Path::new("/p")).unwrap_err();
    let forbidden = daemon.work_snapshot().unwrap_err();
    let unknown = daemon.govern_snapshot().unwrap_err();

    server.join().unwrap();
    assert_eq!(not_found.code, ErrorCode::NotFound);
    assert_eq!(not_found.message, "no such approval");
    assert_eq!(forbidden.code, ErrorCode::Forbidden);
    assert_eq!(unknown.code, ErrorCode::Daemon);
}

#[test]
fn http_failures_are_never_reported_as_success() {
    let (url, server) = spawn_daemon(vec![
        reply(401, "Unauthorized"),
        reply(403, "Forbidden"),
        reply(500, "boom"),
    ]);
    let daemon = client(&url);

    assert_eq!(
        daemon.work_snapshot().unwrap_err().code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        daemon.work_snapshot().unwrap_err().code,
        ErrorCode::Unauthorized
    );
    assert_eq!(daemon.work_snapshot().unwrap_err().code, ErrorCode::Daemon);
    server.join().unwrap();
}

#[test]
fn an_unreachable_daemon_fails_closed() {
    let error = client(&closed_port_url()).work_snapshot().unwrap_err();

    assert_eq!(error.code, ErrorCode::DaemonUnreachable);
}

#[test]
fn malformed_responses_never_become_data() {
    let (url, server) = spawn_daemon(vec![
        reply(200, "<html>not json</html>"),
        reply(200, "{}"),
        reply(200, r#"{"status":"ok"}"#),
        reply(200, r#"{"status":"ok","work":123}"#),
        reply(200, r#"{"status":"error"}"#),
    ]);
    let daemon = client(&url);

    for _ in 0..5 {
        let error = daemon.work_snapshot().unwrap_err();
        assert_eq!(error.code, ErrorCode::Daemon, "{error}");
    }
    server.join().unwrap();
}

#[cfg(unix)]
#[test]
fn a_project_path_that_is_not_utf8_is_refused_locally() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let bad = PathBuf::from(OsString::from_vec(vec![b'/', 0xff, 0xfe]));

    let error = client(&closed_port_url())
        .handoff_receipt("a-1", &bad)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::InvalidInput);
}
