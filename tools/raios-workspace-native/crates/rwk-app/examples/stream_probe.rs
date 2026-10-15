//! Tails the daemon's typed event stream with the real client (`WebSocketEvents`) and prints
//! every control event as it arrives. Read-only: nothing is sent to the daemon beyond the
//! WebSocket upgrade and the bearer token, so it is safe against a running daemon.
//!
//! `stream_probe [seconds]` — tails for that many seconds (default 10). The token is read from
//! `$XDG_CONFIG_HOME/raios/.session_token`, the same file `rwk-app` reads.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rwk_core::control::daemon::{DaemonClient, TokenSource};
use rwk_core::control::stream::WebSocketEvents;
use rwk_core::control::EventSource;
use rwk_core::error::Result;

struct FileToken(PathBuf);
impl TokenSource for FileToken {
    fn bearer_token(&self) -> Result<String> {
        Ok(std::fs::read_to_string(&self.0)
            .unwrap_or_default()
            .trim()
            .to_owned())
    }
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .map(|raw| raw.parse().expect("seconds"))
        .unwrap_or(10);
    let config = std::env::var("XDG_CONFIG_HOME").expect("XDG_CONFIG_HOME");
    let mut source = WebSocketEvents::new(
        format!("http://127.0.0.1:{}", DaemonClient::DEFAULT_PORT),
        Box::new(FileToken(format!("{config}/raios/.session_token").into())),
    );
    println!("stream: {}", source.url());
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        match source.next_event(Duration::from_millis(250)) {
            Ok(Some(event)) => {
                println!("{:>5}ms  {event:?}", started.elapsed().as_millis());
            }
            Ok(None) => {}
            Err(error) => {
                println!("error: {error:?}");
                return;
            }
        }
    }
    println!("done ({seconds} s)");
}
