//! Coalesces wake-ups from the parser thread into at most one pending UI notification.

use std::sync::atomic::{AtomicBool, Ordering};

/// One pending-notification flag shared by the parser thread and the UI.
#[derive(Debug, Default)]
pub struct FrameGate {
    pending: AtomicBool,
}

impl FrameGate {
    /// A gate with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a notification as wanted. Returns true only for the call that must schedule it.
    pub fn request(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    /// Called by the UI right before it takes a snapshot: output arriving afterwards re-arms.
    pub fn take(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::FrameGate;

    #[test]
    fn only_the_first_request_after_a_take_schedules() {
        let gate = FrameGate::new();
        assert!(gate.request());
        assert!(!gate.request());
        assert!(!gate.request());
        gate.take();
        assert!(gate.request());
    }
}
