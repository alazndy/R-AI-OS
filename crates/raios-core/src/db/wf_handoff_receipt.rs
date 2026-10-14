use rusqlite::{params, Connection, OptionalExtension, Result};

/// Delivery state of one queued handoff, read from the control-plane tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffReceipt {
    pub approval_id: String,
    pub project_path: String,
    pub from_agent: String,
    pub to_agent: String,
    /// `pending`, `approved`, `rejected` or `expired`.
    pub status: String,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<String>,
}

impl HandoffReceipt {
    /// True only when the addressed agent's own wrapper consumed the handoff. An approval
    /// resolved by any other principal (for example the control-plane owner) is not delivery.
    pub fn delivered_by_target(&self) -> bool {
        self.status == "approved" && self.resolved_by.as_deref() == Some(self.to_agent.as_str())
    }
}

/// Reads one handoff receipt, scoped to the run contract's workspace root. Returns `None`
/// when the approval does not exist or belongs to a different project.
pub fn cp_handoff_receipt(
    conn: &Connection,
    approval_id: &str,
    project_path: &str,
) -> Result<Option<HandoffReceipt>> {
    let row = conn
        .query_row(
            "SELECT ap.id, contract.workspace_root, t.assignee_id, ap.status,
                    ap.resolved_by, ap.resolved_at, art.metadata_json
             FROM cp_approvals ap
             JOIN cp_tasks t ON t.id = ap.task_id
             JOIN cp_artifacts art ON art.id = ap.artifact_id
             JOIN cp_agent_runs run ON run.id = ap.agent_run_id
             JOIN cp_run_contracts contract ON contract.id = run.run_contract_id
             WHERE ap.id = ?1 AND ap.approval_type = 'handover'
               AND contract.workspace_root = ?2",
            params![approval_id, project_path],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;

    let Some((approval_id, project_path, assignee, status, resolved_by, resolved_at, metadata)) =
        row
    else {
        return Ok(None);
    };
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata).unwrap_or(serde_json::Value::Null);
    Ok(Some(HandoffReceipt {
        approval_id,
        project_path,
        from_agent: metadata["from"].as_str().unwrap_or_default().to_string(),
        to_agent: assignee.unwrap_or_default(),
        status,
        resolved_by,
        resolved_at,
    }))
}
