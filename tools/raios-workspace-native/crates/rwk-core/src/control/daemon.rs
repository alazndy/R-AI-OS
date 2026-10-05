//! HTTP client for the daemon's `/api/v1/control/query` and `/api/v1/control/command` routes.

use std::path::Path;

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, WorkSnapshot};

use super::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
    QueuedHandoff, RegistrationOutcome,
};
use crate::error::Result;

/// Supplies the loopback session bearer token the daemon's auth middleware validates.
/// The production implementation reads the daemon's session token file (P1b).
pub trait TokenSource: Send + Sync {
    /// Returns a currently valid bearer token.
    fn bearer_token(&self) -> Result<String>;
}

/// Blocking HTTP client for the daemon. Call it from a background executor, never from the
/// UI thread.
pub struct DaemonClient {
    base_url: String,
    // Read by the P1b implementation.
    #[allow(dead_code)]
    tokens: Box<dyn TokenSource>,
}

impl DaemonClient {
    /// Default loopback port of the daemon's HTTP API (`http_port` in `raios-policy.toml`).
    pub const DEFAULT_PORT: u16 = 42071;

    /// Creates a client for `base_url`, for example `http://127.0.0.1:42071`.
    pub fn new(base_url: impl Into<String>, tokens: Box<dyn TokenSource>) -> Self {
        Self {
            base_url: base_url.into(),
            tokens,
        }
    }

    /// The base URL requests are sent to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl ControlPlane for DaemonClient {
    fn work_snapshot(&self) -> Result<WorkSnapshot> {
        unimplemented!("P1b")
    }

    fn govern_snapshot(&self) -> Result<GovernSnapshot> {
        unimplemented!("P1b")
    }

    fn command_policy(&self, _command_name: &str) -> Result<PolicyAction> {
        unimplemented!("P1b")
    }

    fn register_project(
        &self,
        _project_root: &Path,
        _key: &IdempotencyKey,
    ) -> Result<RegistrationOutcome> {
        unimplemented!("P1b")
    }

    fn launch_agent(&self, _request: &LaunchAgentRequest) -> Result<()> {
        unimplemented!("P1b")
    }

    fn create_handoff(&self, _request: &CreateHandoffRequest) -> Result<QueuedHandoff> {
        unimplemented!("P1b")
    }

    fn handoff_receipt(
        &self,
        _approval_id: &str,
        _project_root: &Path,
    ) -> Result<HandoffReceiptDto> {
        unimplemented!("P1b")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;

    struct FixedToken;

    impl TokenSource for FixedToken {
        fn bearer_token(&self) -> Result<String> {
            Ok("token".into())
        }
    }

    #[test]
    fn the_client_remembers_its_base_url() {
        let client = DaemonClient::new("http://127.0.0.1:42071", Box::new(FixedToken));
        assert_eq!(client.base_url(), "http://127.0.0.1:42071");
    }

    #[test]
    fn the_default_port_matches_the_shipped_policy() {
        assert_eq!(DaemonClient::DEFAULT_PORT, 42071);
    }
}
