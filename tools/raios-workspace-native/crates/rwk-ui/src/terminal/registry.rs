//! Routes the PTY host's per-session wake-ups to the views' frame gates.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use rwk_core::sessions::SessionId;

use super::gate::FrameGate;

type Entry = (Arc<FrameGate>, async_channel::Sender<()>);

/// One frame gate and wake channel per session. The wake closure the PTY host calls only looks
/// the session up and flips its gate: it runs on a parser thread and must stay cheap.
#[derive(Default)]
pub struct GateRegistry {
    entries: Mutex<HashMap<SessionId, Entry>>,
}

impl GateRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called by the host's wake closure. A session that is not registered (yet, or any more)
    /// is ignored: its first paint takes a snapshot anyway.
    pub fn wake(&self, id: &SessionId) {
        let found = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned();
        if let Some((gate, sender)) = found {
            if gate.request() {
                let _ = sender.try_send(());
            }
        }
    }

    /// Registers a session and returns the gate and the receiver its view listens on.
    pub fn attach(&self, id: &SessionId) -> (Arc<FrameGate>, async_channel::Receiver<()>) {
        let gate = Arc::new(FrameGate::new());
        let (sender, receiver) = async_channel::bounded::<()>(1);
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), (Arc::clone(&gate), sender));
        (gate, receiver)
    }

    /// Forgets a session.
    pub fn detach(&self, id: &SessionId) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wake_reaches_the_attached_session_once_per_frame() {
        let registry = GateRegistry::new();
        let id = SessionId::new("s1");
        let (gate, receiver) = registry.attach(&id);
        registry.wake(&id);
        registry.wake(&id);
        assert_eq!(receiver.try_recv(), Ok(()));
        assert!(receiver.try_recv().is_err(), "wakes coalesce");
        gate.take();
        registry.wake(&id);
        assert_eq!(receiver.try_recv(), Ok(()));
    }

    #[test]
    fn unknown_and_detached_sessions_are_ignored() {
        let registry = GateRegistry::new();
        registry.wake(&SessionId::new("nobody"));
        let id = SessionId::new("s2");
        let (_gate, receiver) = registry.attach(&id);
        registry.detach(&id);
        registry.wake(&id);
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn sessions_do_not_share_a_gate() {
        let registry = GateRegistry::new();
        let (a, b) = (SessionId::new("a"), SessionId::new("b"));
        let (_ga, ra) = registry.attach(&a);
        let (_gb, rb) = registry.attach(&b);
        registry.wake(&a);
        assert_eq!(ra.try_recv(), Ok(()));
        assert!(rb.try_recv().is_err());
    }
}
