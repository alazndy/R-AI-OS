//! UI-local consent for operations whose policy action is `confirm`.
//!
//! The daemon already treats an authenticated command as the confirmation (and audits it as
//! `tool_confirm`), so this registry is a UX safeguard, not a security boundary: it makes the
//! user see exactly what is about to be sent. One request may be pending at a time, a request
//! expires after [`CONSENT_TIMEOUT`], and cancel, expiry or shutdown start nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::error::{AppError, ErrorCode, Result};

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

#[derive(Debug)]
struct PendingConsent {
    sender: Sender<ConsentDecision>,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct ConsentState {
    requests: HashMap<String, PendingConsent>,
    closed: bool,
}

/// Tracks the single pending consent request.
#[derive(Debug, Default)]
pub struct ConsentRegistry {
    state: Mutex<ConsentState>,
}

impl ConsentRegistry {
    /// Creates a registry with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, ConsentState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens a request. Fails while another request is pending or after shutdown.
    pub fn begin(
        &self,
        command_name: &str,
        project_root: &Path,
        summary: &str,
    ) -> Result<(ConsentRequest, Receiver<ConsentDecision>)> {
        self.begin_at(command_name, project_root, summary, Instant::now())
    }

    fn begin_at(
        &self,
        command_name: &str,
        project_root: &Path,
        summary: &str,
        now: Instant,
    ) -> Result<(ConsentRequest, Receiver<ConsentDecision>)> {
        let mut state = self.lock();
        if state.closed {
            return Err(closed());
        }
        state.requests.retain(|_, pending| pending.expires_at > now);
        if !state.requests.is_empty() {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "Another operation is waiting for your consent.",
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::channel();
        state.requests.insert(
            id.clone(),
            PendingConsent {
                sender,
                expires_at: now + CONSENT_TIMEOUT,
            },
        );
        let request = ConsentRequest {
            id,
            command_name: command_name.to_owned(),
            project_root: project_root.to_path_buf(),
            summary: summary.to_owned(),
        };
        Ok((request, receiver))
    }

    /// Records the user's answer for a pending, unexpired request; single use.
    pub fn decide(&self, id: &str, decision: ConsentDecision) -> Result<()> {
        self.decide_at(id, decision, Instant::now())
    }

    fn decide_at(&self, id: &str, decision: ConsentDecision, now: Instant) -> Result<()> {
        let mut state = self.lock();
        if state.closed {
            return Err(closed());
        }
        let pending = state.requests.remove(id).ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "No pending consent request matches this ID.",
            )
        })?;
        if pending.expires_at <= now {
            return Err(expired());
        }
        pending.sender.send(decision).map_err(|_| expired())
    }

    /// Waits for the answer to `id`; an expired or shut-down request resolves to an error.
    pub fn wait(&self, id: &str, receiver: Receiver<ConsentDecision>) -> Result<ConsentDecision> {
        let outcome = match receiver.recv_timeout(CONSENT_TIMEOUT) {
            Ok(decision) => Ok(decision),
            Err(RecvTimeoutError::Timeout) => Err(expired()),
            Err(RecvTimeoutError::Disconnected) if self.lock().closed => Err(closed()),
            Err(RecvTimeoutError::Disconnected) => Err(expired()),
        };
        self.lock().requests.remove(id);
        outcome
    }

    /// Wakes every waiter with an error and refuses new requests.
    pub fn shutdown(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.requests.clear();
    }
}

fn expired() -> AppError {
    AppError::new(
        ErrorCode::Conflict,
        "The consent request expired. Start the operation again.",
    )
}

fn closed() -> AppError {
    AppError::new(
        ErrorCode::WorkspaceClosed,
        "Workspace is closing; consent can no longer be requested.",
    )
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

    #[test]
    fn an_expired_request_cannot_be_approved() {
        let registry = ConsentRegistry::new();
        let now = Instant::now();
        let (request, receiver) = registry
            .begin_at("launch_agent", Path::new("/a"), "Start Claude", now)
            .unwrap();

        let error = registry
            .decide_at(
                &request.id,
                ConsentDecision::Approved,
                now + CONSENT_TIMEOUT,
            )
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(
            receiver.recv().is_err(),
            "the waiter must see no decision at all"
        );
    }

    #[test]
    fn a_request_just_before_expiry_can_still_be_approved() {
        let registry = ConsentRegistry::new();
        let now = Instant::now();
        let (request, receiver) = registry
            .begin_at("launch_agent", Path::new("/a"), "Start Claude", now)
            .unwrap();

        registry
            .decide_at(
                &request.id,
                ConsentDecision::Approved,
                now + CONSENT_TIMEOUT - Duration::from_millis(1),
            )
            .unwrap();

        assert_eq!(receiver.recv().unwrap(), ConsentDecision::Approved);
    }

    #[test]
    fn an_expired_request_does_not_block_the_next_one() {
        let registry = ConsentRegistry::new();
        let now = Instant::now();
        let _first = registry
            .begin_at("launch_agent", Path::new("/a"), "First", now)
            .unwrap();

        let later = now + CONSENT_TIMEOUT + Duration::from_secs(1);

        assert!(registry
            .begin_at("launch_agent", Path::new("/b"), "Second", later)
            .is_ok());
    }
}
