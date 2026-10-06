//! In-memory [`ControlPlane`] for tests. Responses are scripted per method and consumed in
//! order; every call is recorded; a call with nothing scripted fails closed instead of
//! inventing an answer.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use raios_contracts::{GovernSnapshot, HandoffReceiptDto, WorkSnapshot};

use super::{
    ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest, PolicyAction,
    QueuedHandoff, RegistrationOutcome,
};
use crate::error::{AppError, ErrorCode, Result};

/// One call the fake received, with its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedCall {
    /// [`ControlPlane::work_snapshot`].
    WorkSnapshot,
    /// [`ControlPlane::govern_snapshot`].
    GovernSnapshot,
    /// [`ControlPlane::command_policy`].
    CommandPolicy {
        /// Command name that was looked up.
        command_name: String,
    },
    /// [`ControlPlane::register_project`].
    RegisterProject {
        /// Directory that was registered.
        project_root: PathBuf,
        /// Retry key.
        key: IdempotencyKey,
    },
    /// [`ControlPlane::launch_agent`].
    LaunchAgent(LaunchAgentRequest),
    /// [`ControlPlane::create_handoff`].
    CreateHandoff(CreateHandoffRequest),
    /// [`ControlPlane::handoff_receipt`].
    HandoffReceipt {
        /// Approval identifier that was read.
        approval_id: String,
        /// Project the read was scoped to.
        project_root: PathBuf,
    },
}

#[derive(Debug, Default)]
struct Script {
    calls: Vec<RecordedCall>,
    work: VecDeque<Result<WorkSnapshot>>,
    govern: VecDeque<Result<GovernSnapshot>>,
    policies: HashMap<String, PolicyAction>,
    registrations: VecDeque<Result<RegistrationOutcome>>,
    launches: VecDeque<Result<()>>,
    handoffs: VecDeque<Result<QueuedHandoff>>,
    receipts: VecDeque<Result<HandoffReceiptDto>>,
}

/// Scriptable test double for the daemon.
#[derive(Debug, Default)]
pub struct FakeControlPlane {
    script: Mutex<Script>,
}

impl FakeControlPlane {
    /// Creates a fake that has no scripted responses yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues the response of the next [`ControlPlane::work_snapshot`] call.
    pub fn push_work_snapshot(&self, response: Result<WorkSnapshot>) -> &Self {
        self.script().work.push_back(response);
        self
    }

    /// Queues the response of the next [`ControlPlane::govern_snapshot`] call.
    pub fn push_govern_snapshot(&self, response: Result<GovernSnapshot>) -> &Self {
        self.script().govern.push_back(response);
        self
    }

    /// Sets the policy action reported for `command_name`; unknown names fail closed.
    pub fn set_policy(&self, command_name: &str, action: PolicyAction) -> &Self {
        self.script()
            .policies
            .insert(command_name.to_owned(), action);
        self
    }

    /// Queues the response of the next [`ControlPlane::register_project`] call.
    pub fn push_registration(&self, response: Result<RegistrationOutcome>) -> &Self {
        self.script().registrations.push_back(response);
        self
    }

    /// Queues the response of the next [`ControlPlane::launch_agent`] call.
    pub fn push_launch(&self, response: Result<()>) -> &Self {
        self.script().launches.push_back(response);
        self
    }

    /// Queues the response of the next [`ControlPlane::create_handoff`] call.
    pub fn push_handoff(&self, response: Result<QueuedHandoff>) -> &Self {
        self.script().handoffs.push_back(response);
        self
    }

    /// Queues the response of the next [`ControlPlane::handoff_receipt`] call.
    pub fn push_receipt(&self, response: Result<HandoffReceiptDto>) -> &Self {
        self.script().receipts.push_back(response);
        self
    }

    /// Every call received so far, in order, including calls that failed.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.script().calls.clone()
    }
}

fn unscripted(call: &str) -> AppError {
    AppError::new(
        ErrorCode::Internal,
        format!("FakeControlPlane: no scripted response for {call}"),
    )
}

impl ControlPlane for FakeControlPlane {
    fn work_snapshot(&self) -> Result<WorkSnapshot> {
        let mut script = self.script();
        script.calls.push(RecordedCall::WorkSnapshot);
        script
            .work
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("work_snapshot")))
    }

    fn govern_snapshot(&self) -> Result<GovernSnapshot> {
        let mut script = self.script();
        script.calls.push(RecordedCall::GovernSnapshot);
        script
            .govern
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("govern_snapshot")))
    }

    fn command_policy(&self, command_name: &str) -> Result<PolicyAction> {
        let mut script = self.script();
        script.calls.push(RecordedCall::CommandPolicy {
            command_name: command_name.to_owned(),
        });
        script
            .policies
            .get(command_name)
            .copied()
            .ok_or_else(|| unscripted(&format!("command_policy({command_name})")))
    }

    fn register_project(
        &self,
        project_root: &Path,
        key: &IdempotencyKey,
    ) -> Result<RegistrationOutcome> {
        let mut script = self.script();
        script.calls.push(RecordedCall::RegisterProject {
            project_root: project_root.to_path_buf(),
            key: key.clone(),
        });
        script
            .registrations
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("register_project")))
    }

    fn launch_agent(&self, request: &LaunchAgentRequest) -> Result<()> {
        let mut script = self.script();
        script
            .calls
            .push(RecordedCall::LaunchAgent(request.clone()));
        script
            .launches
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("launch_agent")))
    }

    fn create_handoff(&self, request: &CreateHandoffRequest) -> Result<QueuedHandoff> {
        let mut script = self.script();
        script
            .calls
            .push(RecordedCall::CreateHandoff(request.clone()));
        script
            .handoffs
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("create_handoff")))
    }

    fn handoff_receipt(&self, approval_id: &str, project_root: &Path) -> Result<HandoffReceiptDto> {
        let mut script = self.script();
        script.calls.push(RecordedCall::HandoffReceipt {
            approval_id: approval_id.to_owned(),
            project_root: project_root.to_path_buf(),
        });
        script
            .receipts
            .pop_front()
            .unwrap_or_else(|| Err(unscripted("handoff_receipt")))
    }
}
