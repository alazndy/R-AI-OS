//! In-memory [`ControlPlane`] for tests. P1b adds scripted responses and call recording.

use std::path::Path;

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, WorkSnapshot};

use super::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
    QueuedHandoff, RegistrationOutcome,
};
use crate::error::Result;

/// Scriptable test double for the daemon.
#[derive(Debug, Default)]
pub struct FakeControlPlane;

impl FakeControlPlane {
    /// Creates a fake that has no scripted responses yet.
    pub fn new() -> Self {
        Self
    }
}

impl ControlPlane for FakeControlPlane {
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
