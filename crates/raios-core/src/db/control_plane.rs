use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
pub struct SchedulerTask {
    pub id: String,
    pub title: String,
    pub priority: i64,
    /// "task_graph" | "swarm" | "file_approval" | "personal"
    pub execution_kind: String,
    /// For task_graph: the graph_id (= plan_id). For others: None.
    pub plan_id: Option<String>,
    pub assignee_id: Option<String>,
}

/// List all cp_tasks with status='ready', ordered by priority DESC then created_at ASC.
/// Excludes tasks with an unresolved pending approval gate.
/// Tasks whose provider-level token budget is hard-blocked are moved to 'blocked' status.
/// Tasks whose assigned provider lacks required capabilities are also excluded (soft-deferred).
pub fn cp_scheduler_list_ready(conn: &Connection) -> Result<Vec<SchedulerTask>> {
    let sql = format!(
        "SELECT t.id, t.title, t.priority, {origin} AS execution_kind, t.plan_id, t.assignee_id
         FROM cp_tasks t
         WHERE t.status = 'ready'
           AND NOT EXISTS (
             SELECT 1 FROM cp_approvals ap
             WHERE ap.task_id = t.id AND ap.status = 'pending'
           )
         ORDER BY t.priority DESC, t.created_at ASC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let candidates: Vec<SchedulerTask> = stmt
        .query_map([], |row| {
            Ok(SchedulerTask {
                id: row.get(0)?,
                title: row.get(1)?,
                priority: row.get(2)?,
                execution_kind: row.get(3)?,
                plan_id: row.get(4)?,
                assignee_id: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut runnable = Vec::new();
    for task in candidates {
        // Check provider-level budget gate
        let gate = task
            .assignee_id
            .as_deref()
            .map(|provider| cp_check_provider_budget_gate(conn, provider))
            .transpose()?
            .unwrap_or(BudgetGate::Allow);

        if gate.is_blocked() {
            let _ = conn.execute(
                "UPDATE cp_tasks SET status='blocked', updated_at=?1 WHERE id=?2",
                params![now, task.id],
            );
            continue;
        }

        // Check provider capability gate: agent tasks need tool_calling support.
        let needs_tools = matches!(task.execution_kind.as_str(), "swarm" | "task_graph");
        if needs_tools {
            if let Some(provider) = task.assignee_id.as_deref() {
                let capable =
                    cp_check_provider_supports(conn, provider, true, false, false).unwrap_or(true);
                if !capable {
                    // Soft-defer: leave task in 'ready' but skip this scheduling cycle
                    continue;
                }
            }
        }

        runnable.push(task);
    }
    Ok(runnable)
}

/// Returns `(graph_id, node_id)` for a task that belongs to a task graph node.
pub fn cp_task_graph_node_ids(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT graph_id, node_id FROM cp_task_graph_nodes WHERE task_id = ?1",
        params![task_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
}

/// Returns the shell_cmd stored in cp_task_graph_nodes for a task.
pub fn cp_task_graph_shell_cmd(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT shell_cmd FROM cp_task_graph_nodes WHERE task_id = ?1",
        params![task_id],
        |r| r.get(0),
    )
    .optional()
}

// ── Unified read models ───────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct UnifiedTaskRow {
    pub id: String,
    pub title: String,
    pub status: String,
    /// "personal" | "task_graph" | "file_approval" | "swarm"
    pub origin: String,
    pub assignee_id: Option<String>,
    pub project_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalInboxRow {
    pub id: String,
    pub approval_type: String,
    pub reason: String,
    pub status: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
    pub requested_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RunOverviewRow {
    pub id: String,
    pub task_id: String,
    pub task_title: Option<String>,
    pub provider: String,
    pub agent_name: String,
    pub status: String,
    pub started_at: Option<String>,
}

const ORIGIN_EXPR: &str = "
    CASE
      WHEN t.plan_id IS NOT NULL THEN 'task_graph'
      WHEN EXISTS (
        SELECT 1 FROM cp_approvals ap
        WHERE ap.task_id = t.id AND ap.approval_type = 'file_write'
      ) THEN 'file_approval'
      WHEN EXISTS (
        SELECT 1 FROM cp_agent_runs ar
        WHERE ar.task_id = t.id AND ar.provider = 'swarm'
      ) THEN 'swarm'
      WHEN EXISTS (
        SELECT 1 FROM cp_approvals ap
        WHERE ap.task_id = t.id AND ap.approval_type = 'handover'
      ) THEN 'agent_handoff'
      ELSE 'personal'
    END";

/// All non-terminal tasks from cp_tasks, ordered by updated_at DESC.
pub fn cp_query_active_tasks(conn: &Connection) -> Result<Vec<UnifiedTaskRow>> {
    let sql = format!(
        "SELECT t.id, t.title, t.status, {origin} AS origin,
                t.assignee_id, li.project_name, t.created_at, t.updated_at
         FROM cp_tasks t
         LEFT JOIN cp_task_list_items li ON li.task_id = t.id
         WHERE t.status NOT IN ('cancelled', 'completed', 'failed')
         ORDER BY t.updated_at DESC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(UnifiedTaskRow {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            origin: row.get(3)?,
            assignee_id: row.get(4)?,
            project_name: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// All blocked tasks (status = 'blocked').
pub fn cp_query_blocked_tasks(conn: &Connection) -> Result<Vec<UnifiedTaskRow>> {
    let sql = format!(
        "SELECT t.id, t.title, t.status, {origin} AS origin,
                t.assignee_id, li.project_name, t.created_at, t.updated_at
         FROM cp_tasks t
         LEFT JOIN cp_task_list_items li ON li.task_id = t.id
         WHERE t.status = 'blocked'
         ORDER BY t.updated_at DESC",
        origin = ORIGIN_EXPR
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(UnifiedTaskRow {
            id: row.get(0)?,
            title: row.get(1)?,
            status: row.get(2)?,
            origin: row.get(3)?,
            assignee_id: row.get(4)?,
            project_name: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    rows.collect()
}

/// All pending approvals, joined with task titles.
pub fn cp_query_pending_approvals(conn: &Connection) -> Result<Vec<ApprovalInboxRow>> {
    let mut stmt = conn.prepare(
        "SELECT ap.id, ap.approval_type, ap.reason, ap.status,
                ap.task_id, t.title, ap.requested_at
         FROM cp_approvals ap
         LEFT JOIN cp_tasks t ON t.id = ap.task_id
         WHERE ap.status = 'pending'
         ORDER BY ap.requested_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ApprovalInboxRow {
            id: row.get(0)?,
            approval_type: row.get(1)?,
            reason: row.get(2)?,
            status: row.get(3)?,
            task_id: row.get(4)?,
            task_title: row.get(5)?,
            requested_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

/// All in-progress agent runs.
pub fn cp_query_active_runs(conn: &Connection) -> Result<Vec<RunOverviewRow>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.task_id, t.title, r.provider, r.agent_name, r.status, r.started_at
         FROM cp_agent_runs r
         LEFT JOIN cp_tasks t ON t.id = r.task_id
         WHERE r.status IN ('pending', 'starting', 'running', 'awaiting_input', 'awaiting_approval')
         ORDER BY r.started_at DESC NULLS LAST",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(RunOverviewRow {
            id: row.get(0)?,
            task_id: row.get(1)?,
            task_title: row.get(2)?,
            provider: row.get(3)?,
            agent_name: row.get(4)?,
            status: row.get(5)?,
            started_at: row.get(6)?,
        })
    })?;
    rows.collect()
}

// ── Daemon snapshot ───────────────────────────────────────────────────────────

/// Point-in-time operational snapshot of the canonical control plane.
#[derive(Debug, Default)]
pub struct DaemonSnapshot {
    /// All non-terminal tasks (queued / ready / running / blocked).
    pub active_tasks: Vec<UnifiedTaskRow>,
    /// All in-progress or pending agent runs.
    pub active_runs: Vec<RunOverviewRow>,
    /// All approvals waiting for a human decision.
    pub pending_approvals: Vec<ApprovalInboxRow>,
    /// Tasks explicitly in 'blocked' status.
    pub blocked_tasks: Vec<UnifiedTaskRow>,
    /// Provider names whose budget is strained (SoftDefer) or exhausted (HardBlock).
    pub budget_deferrals: Vec<String>,
}

pub fn cp_daemon_snapshot(conn: &Connection) -> Result<DaemonSnapshot> {
    let active_tasks = cp_query_active_tasks(conn)?;
    let active_runs = cp_query_active_runs(conn)?;
    let pending_approvals = cp_query_pending_approvals(conn)?;
    let blocked_tasks = cp_query_blocked_tasks(conn)?;

    // Collect providers with a non-Allow budget gate
    let mut budget_deferrals = Vec::new();
    let mut prov_stmt =
        conn.prepare("SELECT DISTINCT provider FROM cp_budget_ledger WHERE provider IS NOT NULL")?;
    let providers: Vec<String> = prov_stmt
        .query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for provider in providers {
        match cp_check_provider_budget_gate(conn, &provider)? {
            BudgetGate::Allow | BudgetGate::AllowUnknown => {}
            _ => budget_deferrals.push(provider),
        }
    }

    Ok(DaemonSnapshot {
        active_tasks,
        active_runs,
        pending_approvals,
        blocked_tasks,
        budget_deferrals,
    })
}

// ── Legacy cache repair tools ─────────────────────────────────────────────────

/// Result of a divergence check between canonical state and a legacy cache.
#[derive(Debug, Default)]
pub struct DriftReport {
    /// Task ids present in canonical cp_tasks but missing from legacy cache.
    pub missing_from_cache: Vec<String>,
    /// Task ids present in legacy cache but missing from canonical cp_tasks.
    pub stale_in_cache: Vec<String>,
}

/// Check for drift between cp_tasks (canonical) and task_graph_nodes (cache) for a given graph.
pub fn cp_detect_graph_cache_drift(conn: &Connection, graph_id: &str) -> Result<DriftReport> {
    // canonical task ids for this graph
    let mut canonical_stmt =
        conn.prepare("SELECT task_id FROM cp_task_graph_nodes WHERE graph_id = ?1")?;
    let canonical_ids: std::collections::HashSet<String> = canonical_stmt
        .query_map(params![graph_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // legacy cache node ids for this graph (via cp_task_id column if present)
    let mut cache_stmt = conn.prepare(
        "SELECT cp_task_id FROM task_graph_nodes WHERE graph_id = ?1 AND cp_task_id IS NOT NULL",
    )?;
    let cache_ids: std::collections::HashSet<String> = cache_stmt
        .query_map(params![graph_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let missing_from_cache: Vec<String> = canonical_ids.difference(&cache_ids).cloned().collect();
    let stale_in_cache: Vec<String> = cache_ids.difference(&canonical_ids).cloned().collect();

    Ok(DriftReport {
        missing_from_cache,
        stale_in_cache,
    })
}

/// Rebuild the legacy task_graph_nodes cache for all graphs from canonical cp_* state.
/// Safe to call multiple times (idempotent).
pub fn cp_rebuild_task_graph_cache(conn: &Connection) -> Result<usize> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut rebuilt = 0usize;

    // For each cp_task_graph_nodes row, sync status from cp_tasks into task_graph_nodes
    let mut stmt = conn.prepare(
        "SELECT cgn.graph_id, cgn.node_id, cgn.task_id, t.status, t.description
         FROM cp_task_graph_nodes cgn
         JOIN cp_tasks t ON t.id = cgn.task_id",
    )?;

    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (graph_id, node_id, _task_id, cp_status, _desc) in &rows {
        let legacy_status = match cp_status.as_str() {
            "completed" => "completed",
            "failed" | "cancelled" => "failed",
            "running" => "running",
            _ => "pending",
        };
        let updated = conn.execute(
            "UPDATE task_graph_nodes SET status = ?1
             WHERE graph_id = ?2 AND id = ?3 AND status != ?1",
            params![legacy_status, graph_id, node_id],
        )?;
        if updated > 0 {
            rebuilt += 1;
        }
    }

    // Also sync task_graphs status from cp_task_graphs
    conn.execute(
        "UPDATE task_graphs SET status = cg.status, completed_at = ?1
         FROM cp_task_graphs cg
         WHERE task_graphs.id = cg.graph_id
           AND cg.completed_at IS NOT NULL
           AND task_graphs.completed_at IS NULL",
        params![now],
    )
    .ok(); // best-effort; fails on older SQLite that doesn't support UPDATE FROM

    Ok(rebuilt)
}
