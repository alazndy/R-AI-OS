//! Process-wide typed event bus for the resident daemon.
//!
//! The daemon broadcasts plain JSON strings on a single
//! [`tokio::sync::broadcast`] channel shared by every transport (daemon TCP,
//! MCP-over-TCP, HTTP/WebSocket, background workers). Historically only loose
//! `{"event": ...}` payloads travelled on that bus; typed
//! [`raios_contracts::Event`] values were confined to the per-connection TCP
//! protocol responses.
//!
//! This module adds the missing typed producer path: the daemon installs the
//! shared sender once at start-up, and any code running inside the daemon
//! process can call [`emit`] to publish a typed event on the same bus.
//! Existing consumers are unaffected — the log ring buffer keys on
//! `v["event"]`, and listeners that already parse typed events simply start
//! receiving them.
//!
//! `emit` is deliberately a no-op when no bus has been installed: wrapper and
//! CLI processes that run control-plane dispatch in-process (for example the
//! `raios run` session wrapper) have no daemon transports to fan out to.

use raios_contracts::Event;
use std::sync::RwLock;
use tokio::sync::broadcast;

static BUS: RwLock<Option<broadcast::Sender<String>>> = RwLock::new(None);

/// Installs (or replaces) the process-wide bus used by [`emit`].
///
/// The daemon calls this with the same sender every transport subscribes to.
/// Tests install their own sender; the last install wins.
pub(crate) fn install(tx: &broadcast::Sender<String>) {
    let mut bus = BUS.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    *bus = Some(tx.clone());
}

/// Serializes `event` and broadcasts it on the installed bus.
///
/// No-op when no daemon bus is installed. Serialization of [`Event`] cannot
/// fail in practice (strings and JSON values only); a failure is dropped
/// rather than panicking a control-plane mutation mid-commit, and a send with
/// no live subscriber is not an error for a fire-and-forget bus.
pub(crate) fn emit(event: Event) {
    let Ok(payload) = serde_json::to_string(&event) else {
        return;
    };
    let bus = BUS.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(tx) = bus.as_ref() {
        let _ = tx.send(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_broadcasts_typed_json() {
        // The bus is process-wide, so tests that install it serialize on the
        // crate's shared global-state lock and filter for their own events —
        // other tests may dispatch control-plane commands on this same bus in
        // parallel.
        let _lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (tx, mut rx) = broadcast::channel::<String>(16);
        install(&tx);
        emit(Event::AgentRunStateChanged {
            run_id: "run-typed-bus".into(),
            agent_name: "claude".into(),
            status: "running".into(),
        });

        let mut seen = None;
        while let Ok(msg) = rx.try_recv() {
            if msg.contains("run-typed-bus") {
                seen = Some(msg);
                break;
            }
        }
        let payload: serde_json::Value =
            serde_json::from_str(&seen.expect("typed event must arrive on the bus")).unwrap();
        assert_eq!(payload["event_type"], "AgentRunStateChanged");
        assert_eq!(payload["payload"]["run_id"], "run-typed-bus");
        assert_eq!(payload["payload"]["agent_name"], "claude");
        assert_eq!(payload["payload"]["status"], "running");
    }

    #[test]
    fn emit_never_panics_regardless_of_bus_state() {
        // Intentionally lock-free: whether another test installed a bus or the
        // bus is still empty, emitting must never panic.
        emit(Event::ApprovalResolved {
            approval_id: "not-installed".into(),
            status: "approved".into(),
        });
    }
}
