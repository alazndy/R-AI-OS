//! Response DTOs for handoff receipts and command-policy lookups.

use serde::{Deserialize, Serialize};

/// Delivery receipt of one queued handoff, scoped to a project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffReceiptDto {
    /// Approval identifier returned when the handoff was queued.
    pub approval_id: String,
    /// Project root the handoff is scoped to.
    pub project_path: String,
    /// Agent identity that queued the handoff.
    pub from_agent: String,
    /// Agent identity the handoff is addressed to.
    pub to_agent: String,
    /// Approval state: `"pending"`, `"approved"`, `"rejected"` or `"expired"`.
    pub status: String,
    /// Principal that resolved the approval, when it has been resolved.
    pub resolved_by: Option<String>,
    /// Timestamp of the resolution, when it has been resolved.
    pub resolved_at: Option<String>,
    /// True only when the addressed agent's own wrapper consumed the handoff
    /// (`status == "approved"` and `resolved_by == to_agent`). A human approval
    /// through `ApproveHandoff` is not delivery.
    pub delivered_by_target: bool,
}

/// Policy action the daemon applies to one control command.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandPolicyDto {
    /// Snake-case command name, for example `"launch_agent"`.
    pub command_name: String,
    /// `"allow"`, `"confirm"` or `"deny"`.
    pub action: String,
}
