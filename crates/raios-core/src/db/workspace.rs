//! Exact project-scoped projection for the native desktop workspace.
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WorkspaceTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub assignee: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceApproval {
    pub id: String,
    pub task_id: Option<String>,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceSnapshot {
    pub tasks: Vec<WorkspaceTask>,
    pub approvals: Vec<WorkspaceApproval>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceHandoffState {
    pub status: String,
    pub resolved_by: Option<String>,
}

/// Receipt state is scoped independently of the client's supplied approval ID.
pub fn cp_workspace_handoff_state(
    conn: &Connection,
    canonical_root: &str,
    approval_id: &str,
) -> Result<Option<WorkspaceHandoffState>> {
    conn.query_row(
        "SELECT a.status, a.resolved_by FROM cp_approvals a
         WHERE a.id = ?2 AND a.approval_type = 'handover'
           AND (a.project_id IN (SELECT id FROM projects WHERE path = ?1)
                OR EXISTS (SELECT 1 FROM cp_run_contracts c WHERE c.task_id = a.task_id AND c.workspace_root = ?1))",
        params![canonical_root, approval_id], |r| Ok(WorkspaceHandoffState { status: r.get(0)?, resolved_by: r.get(1)? }),
    ).optional()
}

/// Ambiguous global and name-only records are deliberately omitted. This is a
/// read projection of existing state; opening it does not create a second store.
pub fn cp_workspace_snapshot(conn: &Connection, canonical_root: &str) -> Result<WorkspaceSnapshot> {
    let tx = conn.unchecked_transaction()?;
    let tasks = {
        let mut statement = tx.prepare(
            "SELECT t.id, t.title, t.status, t.assignee_id FROM cp_tasks t
             WHERE t.status NOT IN ('completed', 'cancelled', 'failed')
               AND (t.project_id IN (SELECT id FROM projects WHERE path = ?1)
                    OR EXISTS (SELECT 1 FROM cp_run_contracts c
                               WHERE c.task_id = t.id AND c.workspace_root = ?1))
             ORDER BY t.updated_at DESC LIMIT 100",
        )?;
        let rows = statement.query_map(params![canonical_root], |r| {
            Ok(WorkspaceTask {
                id: r.get(0)?,
                title: r.get(1)?,
                status: r.get(2)?,
                assignee: r.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };
    let approvals = {
        let mut statement = tx.prepare(
            "SELECT a.id, a.task_id, a.approval_type, a.reason FROM cp_approvals a
             WHERE a.status = 'pending'
               AND (a.project_id IN (SELECT id FROM projects WHERE path = ?1)
                    OR EXISTS (SELECT 1 FROM cp_run_contracts c
                               WHERE c.task_id = a.task_id AND c.workspace_root = ?1))
             ORDER BY a.requested_at DESC LIMIT 100",
        )?;
        let rows = statement.query_map(params![canonical_root], |r| {
            Ok(WorkspaceApproval {
                id: r.get(0)?,
                task_id: r.get(1)?,
                kind: r.get(2)?,
                reason: r.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };
    tx.commit()?;
    Ok(WorkspaceSnapshot { tasks, approvals })
}
