//! `WebSocketEvents` against a scripted local WebSocket server.
//!
//! The server is a real socket with a real handshake: a fake would not prove that the bearer
//! token travels in the upgrade request, that a dropped socket is reported and retried, or
//! that a refused port turns into a backoff instead of a panic.

use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rwk_core::control::daemon::TokenSource;
use rwk_core::control::stream::WebSocketEvents;
use rwk_core::control::{ConnectionState, ControlEvent, EventSource};
use rwk_core::error::{AppError, ErrorCode, Result};
use serde_json::json;
use tungstenite::handshake::server::Request;
use tungstenite::{Message, WebSocket};

/// A token source that always answers the same token.
struct StaticToken(&'static str);

impl TokenSource for StaticToken {
    fn bearer_token(&self) -> Result<String> {
        Ok(self.0.to_owned())
    }
}

/// A token source that fails like a missing or expired token file.
struct BrokenToken;

impl TokenSource for BrokenToken {
    fn bearer_token(&self) -> Result<String> {
        Err(AppError::new(ErrorCode::Unauthorized, "jeton yok"))
    }
}

/// What one handshake carried; the daemon's auth middleware requires both headers.
#[derive(Debug)]
struct Handshake {
    authorization: String,
    host: String,
}

/// One per-connection script: what the scripted daemon does after the handshake.
type Script = Box<dyn FnOnce(&mut WebSocket<TcpStream>) + Send>;

/// Serves one connection per script and returns the captured handshakes once all were served.
fn spawn_stream_server(scripts: Vec<Script>) -> (String, JoinHandle<Vec<Handshake>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("local addr");
    let handle = thread::spawn(move || {
        let mut handshakes = Vec::new();
        for script in scripts {
            let (stream, _) = listener.accept().expect("accept");
            let captured = Arc::new(Mutex::new(None));
            let sink = Arc::clone(&captured);
            // The allow is for tungstenite's own handshake callback shape, not for our code:
            // `accept_hdr` forces `Result<Response, ErrorResponse>` whose Err is 136 bytes.
            #[allow(clippy::result_large_err)]
            let mut socket = tungstenite::accept_hdr(stream, move |request: &Request, response| {
                let header = |name: &str| {
                    request
                        .headers()
                        .get(name)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned()
                };
                *sink.lock().expect("lock") = Some(Handshake {
                    authorization: header("Authorization"),
                    host: header("Host"),
                });
                Ok(response)
            })
            .expect("handshake");
            script(&mut socket);
            handshakes.push(
                captured
                    .lock()
                    .expect("lock")
                    .take()
                    .expect("captured handshake"),
            );
        }
        handshakes
    });
    (format!("http://{address}"), handle)
}

/// Drives `next_event` in short slices until an event arrives or `limit` passes.
fn next_within(events: &mut WebSocketEvents, limit: Duration) -> ControlEvent {
    let deadline = Instant::now() + limit;
    loop {
        match events
            .next_event(Duration::from_millis(100))
            .expect("no error")
        {
            Some(event) => return event,
            None => {
                assert!(Instant::now() < deadline, "no event within {limit:?}");
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[test]
fn connects_with_the_bearer_token_and_maps_typed_events() {
    let (base, server) = spawn_stream_server(vec![Box::new(|socket| {
        let frame = json!({
            "event_type": "AgentRunStateChanged",
            "payload": {"run_id": "run-1", "agent_name": "codex_kaira", "status": "running"},
        });
        socket
            .send(Message::Text(frame.to_string()))
            .expect("send frame");
        thread::sleep(Duration::from_millis(300));
    })]);
    let mut events = WebSocketEvents::new(&base, Box::new(StaticToken("token-1")));

    assert_eq!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::ConnectionChanged(ConnectionState::Connected)
    );
    assert_eq!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::RunChanged {
            run_id: "run-1".into(),
            agent_name: "codex_kaira".into(),
            status: "running".into(),
        }
    );

    let handshakes = server.join().expect("server");
    assert_eq!(handshakes[0].authorization, "Bearer token-1");
    assert!(
        !handshakes[0].host.is_empty(),
        "the daemon's auth middleware requires the Host header"
    );
}

#[test]
fn loose_and_unknown_frames_are_skipped_without_events() {
    let (base, server) = spawn_stream_server(vec![Box::new(|socket| {
        socket
            .send(Message::Text(
                json!({"event": "log", "text": "loose"}).to_string(),
            ))
            .expect("send loose frame");
        socket
            .send(Message::Text(
                json!({"event_type": "LogAppended", "payload": {"log": {}}}).to_string(),
            ))
            .expect("send unknown typed frame");
        thread::sleep(Duration::from_millis(800));
    })]);
    let mut events = WebSocketEvents::new(&base, Box::new(StaticToken("token-1")));

    assert_eq!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::ConnectionChanged(ConnectionState::Connected)
    );
    // Both frames were consumed and mapped to nothing: the short slice ends quietly.
    assert_eq!(
        events
            .next_event(Duration::from_millis(150))
            .expect("no error"),
        None
    );
    server.join().expect("server");
}

#[test]
fn a_dropped_socket_is_reported_and_reconnected() {
    let (base, server) = spawn_stream_server(vec![
        // First connection: the daemon drops it right after the handshake.
        Box::new(|_socket| {}),
        // Second connection: an approval arrives.
        Box::new(|socket| {
            let frame = json!({
                "event_type": "ApprovalRequested",
                "payload": {"approval_id": "ap-1", "kind": "handoff", "title": "t", "target": "x"},
            });
            socket
                .send(Message::Text(frame.to_string()))
                .expect("send frame");
            thread::sleep(Duration::from_millis(300));
        }),
    ]);
    let mut events = WebSocketEvents::new(&base, Box::new(StaticToken("token-1")));

    assert_eq!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::ConnectionChanged(ConnectionState::Connected)
    );
    assert!(matches!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::ConnectionChanged(ConnectionState::Unreachable { .. })
    ));
    assert_eq!(
        next_within(&mut events, Duration::from_secs(5)),
        ControlEvent::ConnectionChanged(ConnectionState::Connected)
    );
    assert_eq!(
        next_within(&mut events, Duration::from_secs(3)),
        ControlEvent::ApprovalChanged {
            approval_id: "ap-1".into(),
        }
    );
    server.join().expect("server");
}

#[test]
fn a_refused_port_reports_unreachable_and_waits_before_retrying() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("addr");
    drop(listener);

    let mut events = WebSocketEvents::new(
        format!("http://{address}"),
        Box::new(StaticToken("token-1")),
    );
    assert_eq!(events.url(), format!("ws://{address}/api/stream"));

    assert!(matches!(
        events
            .next_event(Duration::from_millis(500))
            .expect("no error"),
        Some(ControlEvent::ConnectionChanged(
            ConnectionState::Unreachable { .. }
        ))
    ));
    // Inside the backoff window nothing is attempted and no event repeats.
    assert_eq!(
        events
            .next_event(Duration::from_millis(50))
            .expect("no error"),
        None
    );
}

#[test]
fn a_missing_token_fails_closed_without_connecting() {
    let mut events = WebSocketEvents::new("http://127.0.0.1:1", Box::new(BrokenToken));

    let error = events
        .next_event(Duration::from_millis(200))
        .expect_err("a missing token must fail, not connect");

    assert_eq!(error.code, ErrorCode::Unauthorized);
}
