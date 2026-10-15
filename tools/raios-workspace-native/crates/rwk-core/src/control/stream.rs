//! WebSocket source of typed [`ControlEvent`]s: the live half of the inspector.
//!
//! Subscribes to the daemon's `GET /api/stream` socket with the same bearer token the HTTP
//! client uses and maps the typed `Event` frames (the `raios-contracts` `event_type` tag) into
//! [`ControlEvent`]s. The loose `{"event": …}` frames other transports rely on are ignored on
//! purpose: the inspector reacts to the typed contract only.
//!
//! The stream is an accelerator, never a requirement. A refused connection, an expired token,
//! a dropped socket: everything is reported as [`ConnectionState`] transitions and retried
//! with backoff, while the caller keeps its HTTP poll as the fallback. This type never panics
//! on a broken stream and never blocks longer than the timeout its caller passes.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::{client, Error as WsError, Message, WebSocket};

use super::daemon::TokenSource;
use super::{ConnectionState, ControlEvent, EventSource};
use crate::error::Result;

/// The daemon route that broadcasts typed events.
const STREAM_PATH: &str = "/api/stream";
/// Longest a single connection attempt may take, connect and handshake together.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// First pause after a broken stream, doubled up to [`MAX_BACKOFF`].
const FIRST_BACKOFF: Duration = Duration::from_millis(250);
/// Longest pause between reconnect attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(10);
/// Never ask the socket to block for less than this.
const MIN_READ_TIMEOUT: Duration = Duration::from_millis(20);

/// A blocking WebSocket subscription to the daemon's typed event stream.
///
/// [`EventSource::next_event`] answers [`ControlEvent::ConnectionChanged`] when the stream
/// connects or breaks, and the typed variants ([`ControlEvent::RunChanged`],
/// [`ControlEvent::ApprovalChanged`], [`ControlEvent::SnapshotChanged`]) for events worth
/// reacting to. `Err` is reserved for the one failure that cannot be retried locally: an
/// unreadable or expired session token.
pub struct WebSocketEvents {
    url: String,
    tokens: Box<dyn TokenSource>,
    socket: Option<WebSocket<TcpStream>>,
    next_attempt: Option<Instant>,
    backoff: Duration,
    last_reported: Option<ConnectionState>,
}

impl WebSocketEvents {
    /// A source for the daemon at `base_url` — the same `http(s)://host:port` the HTTP client
    /// uses; the scheme is rewritten to `ws(s)://…/api/stream`. `tokens` is read again on
    /// every reconnect because the daemon rotates the token.
    pub fn new(base_url: impl AsRef<str>, tokens: Box<dyn TokenSource>) -> Self {
        Self {
            url: stream_url(base_url.as_ref()),
            tokens,
            socket: None,
            next_attempt: None,
            backoff: FIRST_BACKOFF,
            last_reported: None,
        }
    }

    /// The socket address events are read from.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Reports `state` once; a repeat of the state already reported yields no event.
    fn report(&mut self, state: ConnectionState) -> Option<ControlEvent> {
        if self.last_reported.as_ref() == Some(&state) {
            return None;
        }
        self.last_reported = Some(state.clone());
        Some(ControlEvent::ConnectionChanged(state))
    }

    /// Drops the broken socket, reports the daemon as unreachable and backs off.
    fn unreachable(&mut self, reason: impl Into<String>) -> Option<ControlEvent> {
        self.socket = None;
        self.next_attempt = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
        self.report(ConnectionState::Unreachable {
            reason: reason.into(),
        })
    }

    /// Time left in the backoff window, `None` when connecting may be tried now.
    fn retry_in(&self) -> Option<Duration> {
        let at = self.next_attempt?;
        let now = Instant::now();
        (at > now).then(|| at - now)
    }

    /// Opens one connection with the bearer token; `Err` carries the banner reason.
    fn connect(&self, token: &str) -> std::result::Result<WebSocket<TcpStream>, String> {
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|error| format!("geçersiz akış adresi: {error}"))?;
        let uri = request.uri().clone();
        let host = uri
            .host()
            .ok_or_else(|| "akış adresinde sunucu adı yok".to_owned())?
            .to_owned();
        let port = uri.port_u16().unwrap_or(80);
        let bearer = format!("Bearer {token}");
        request.headers_mut().insert(
            "Authorization",
            bearer
                .parse()
                .map_err(|_| "oturum jetonu başlık olarak geçersiz".to_owned())?,
        );
        let address = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|error| format!("akış adresi çözülemedi: {error}"))?
            .next()
            .ok_or_else(|| "akış adresi çözülemedi".to_owned())?;
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
            .map_err(|error| format!("olay akışına bağlanılamadı: {error}"))?;
        stream
            .set_read_timeout(Some(CONNECT_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(CONNECT_TIMEOUT)))
            .map_err(|error| format!("olay akışı hazırlanamadı: {error}"))?;
        let _ = stream.set_nodelay(true);
        let (socket, _response) = client(request, stream)
            .map_err(|error| format!("olay akışı el sıkışması başarısız: {error}"))?;
        Ok(socket)
    }

    /// Reads frames until one maps to a control event or `timeout` passes.
    fn read_next(&mut self, timeout: Duration) -> Option<ControlEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let socket = self.socket.as_mut()?;
            let _ = socket
                .get_ref()
                .set_read_timeout(Some(remaining.max(MIN_READ_TIMEOUT)));
            let outcome = socket.read();
            match outcome {
                Ok(Message::Text(text)) => {
                    if let Some(event) = event_from_text(&text) {
                        return Some(event);
                    }
                }
                Ok(Message::Ping(payload)) => {
                    if let Some(socket) = self.socket.as_mut() {
                        let _ = socket.send(Message::Pong(payload));
                    }
                }
                Ok(Message::Close(_)) => {
                    return self.unreachable("olay akışı kapandı");
                }
                Ok(_) => {}
                Err(WsError::Io(error))
                    if error.kind() == ErrorKind::WouldBlock
                        || error.kind() == ErrorKind::TimedOut =>
                {
                    return None;
                }
                Err(WsError::Io(error)) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => {
                    return self.unreachable(format!("olay akışı koptu: {error}"));
                }
            }
        }
    }
}

impl EventSource for WebSocketEvents {
    fn next_event(&mut self, timeout: Duration) -> Result<Option<ControlEvent>> {
        if self.socket.is_none() {
            if let Some(wait) = self.retry_in() {
                let pause = wait.min(timeout);
                if pause > Duration::ZERO {
                    thread::sleep(pause);
                }
                return Ok(None);
            }
            let token = self.tokens.bearer_token()?;
            return match self.connect(&token) {
                Ok(socket) => {
                    self.socket = Some(socket);
                    self.next_attempt = None;
                    self.backoff = FIRST_BACKOFF;
                    Ok(self.report(ConnectionState::Connected))
                }
                Err(reason) => Ok(self.unreachable(reason)),
            };
        }
        Ok(self.read_next(timeout))
    }
}

/// Maps one typed daemon frame into a [`ControlEvent`]. Loose frames (`{"event": …}`),
/// unknown event types and malformed JSON return `None`; the caller skips them.
fn event_from_text(text: &str) -> Option<ControlEvent> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let payload = value.get("payload")?;
    match value.get("event_type")?.as_str()? {
        "AgentRunStateChanged" => Some(ControlEvent::RunChanged {
            run_id: payload.get("run_id")?.as_str()?.to_owned(),
            agent_name: payload.get("agent_name")?.as_str()?.to_owned(),
            status: payload.get("status")?.as_str()?.to_owned(),
        }),
        "ApprovalRequested" | "ApprovalResolved" => Some(ControlEvent::ApprovalChanged {
            approval_id: payload.get("approval_id")?.as_str()?.to_owned(),
        }),
        "SnapshotUpdated" => Some(ControlEvent::SnapshotChanged {
            sequence: payload.get("sequence")?.as_u64()?,
        }),
        _ => None,
    }
}

/// Rewrites an HTTP base URL into the WebSocket URL of the event stream.
fn stream_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let socket = match base.split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => base.to_owned(),
    };
    format!("{socket}{STREAM_PATH}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_urls_rewrite_the_scheme_and_append_the_route() {
        assert_eq!(
            stream_url("http://127.0.0.1:42071"),
            "ws://127.0.0.1:42071/api/stream"
        );
        assert_eq!(
            stream_url("http://127.0.0.1:42071/"),
            "ws://127.0.0.1:42071/api/stream"
        );
        assert_eq!(
            stream_url("https://example.test"),
            "wss://example.test/api/stream"
        );
        assert_eq!(
            stream_url("ws://127.0.0.1:1"),
            "ws://127.0.0.1:1/api/stream"
        );
    }

    #[test]
    fn typed_run_state_events_map_to_run_changed() {
        let frame = r#"{"event_type":"AgentRunStateChanged","payload":{"run_id":"r1","agent_name":"codex_kaira","status":"running"}}"#;

        assert_eq!(
            event_from_text(frame),
            Some(ControlEvent::RunChanged {
                run_id: "r1".into(),
                agent_name: "codex_kaira".into(),
                status: "running".into(),
            })
        );
    }

    #[test]
    fn both_approval_events_map_to_approval_changed() {
        for event_type in ["ApprovalRequested", "ApprovalResolved"] {
            let frame = format!(
                r#"{{"event_type":"{event_type}","payload":{{"approval_id":"a1","status":"approved","kind":"handoff","title":"t","target":"x"}}}}"#
            );

            assert_eq!(
                event_from_text(&frame),
                Some(ControlEvent::ApprovalChanged {
                    approval_id: "a1".into(),
                }),
                "{event_type}"
            );
        }
    }

    #[test]
    fn snapshot_updates_carry_their_sequence() {
        let frame = r#"{"event_type":"SnapshotUpdated","payload":{"sequence":7,"timestamp":"t","now":{},"work":{},"explore":{},"govern":{}}}"#;

        assert_eq!(
            event_from_text(frame),
            Some(ControlEvent::SnapshotChanged { sequence: 7 })
        );
    }

    #[test]
    fn loose_unknown_and_malformed_frames_are_ignored() {
        for frame in [
            r#"{"event":"log","text":"loose frame"}"#,
            r#"{"event_type":"LogAppended","payload":{"log":{}}}"#,
            r#"{"event_type":"AgentRunStateChanged","payload":{"run_id":"r1"}}"#,
            r#"{"event_type":"AgentRunStateChanged"}"#,
            r#"{"event_type":"ApprovalRequested","payload":{"approval_id":7}}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(event_from_text(frame), None, "{frame}");
        }
    }
}
