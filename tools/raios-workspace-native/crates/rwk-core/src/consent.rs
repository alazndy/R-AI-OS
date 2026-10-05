//! UI-local consent for operations whose policy action is `confirm`.
//!
//! The daemon already treats an authenticated command as the confirmation (and audits it as
//! `tool_confirm`), so this registry is a UX safeguard, not a security boundary: it makes the
//! user see exactly what is about to be sent. One request may be pending at a time, a request
//! expires after [`CONSENT_TIMEOUT`], and cancel, expiry or shutdown start nothing.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::Result;

/// How long a consent request stays valid.
pub const CONSENT_TIMEOUT: Duration = Duration::from_secs(120);

/// What the dialog shows. `summary` is derived from the exact payload that will be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    /// Single-use request identifier.
    pub id: String,
    /// Snake-case command name, for example `launch_agent`.
    pub command_name: String,
    /// Canonical project root the operation targets.
    pub project_root: PathBuf,
    /// Human-readable operation text.
    pub summary: String,
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDecision {
    /// Send the command.
    Approved,
    /// Send nothing.
    Denied,
}

#[derive(Debug, Default)]
struct ConsentState {
    // Set by the P1b `shutdown` implementation.
    #[allow(dead_code)]
    closed: bool,
}

/// Tracks the single pending consent request.
#[derive(Debug, Default)]
pub struct ConsentRegistry {
    // Read by the P1b implementation.
    #[allow(dead_code)]
    state: Mutex<ConsentState>,
}

impl ConsentRegistry {
    /// Creates a registry with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a request. Fails while another request is pending or after shutdown.
    pub fn begin(
        &self,
        _command_name: &str,
        _project_root: &Path,
        _summary: &str,
    ) -> Result<(ConsentRequest, Receiver<ConsentDecision>)> {
        unimplemented!("P1b")
    }

    /// Records the user's answer for a pending, unexpired request; single use.
    pub fn decide(&self, _id: &str, _decision: ConsentDecision) -> Result<()> {
        unimplemented!("P1b")
    }

    /// Waits for the answer to `id`; an expired or shut-down request resolves to an error.
    pub fn wait(&self, _id: &str, _receiver: Receiver<ConsentDecision>) -> Result<ConsentDecision> {
        unimplemented!("P1b")
    }

    /// Wakes every waiter with an error and refuses new requests.
    pub fn shutdown(&self) {
        unimplemented!("P1b")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_requests_expire_after_two_minutes() {
        assert_eq!(CONSENT_TIMEOUT, Duration::from_secs(120));
    }

    #[test]
    fn a_new_registry_can_be_created_without_side_effects() {
        let _registry = ConsentRegistry::new();
    }
}
