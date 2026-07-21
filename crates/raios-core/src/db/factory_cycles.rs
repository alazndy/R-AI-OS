//! Repository boundary for cycle, stage, change, and impact records.

use rusqlite::{params, OptionalExtension, Result, Transaction};
use uuid::Uuid;

pub const FACTORY_CYCLES_TABLE: &str = "cp_factory_cycles";
pub const FACTORY_STAGE_RUNS_TABLE: &str = "cp_factory_stage_runs";
pub const FACTORY_CHANGE_REQUESTS_TABLE: &str = "cp_factory_change_requests";
pub const FACTORY_IMPACT_ASSESSMENTS_TABLE: &str = "cp_factory_impact_assessments";

pub const FACTORY_LIFECYCLE_STAGES: [&str; 7] = [
    "discover", "define", "design", "build", "verify", "release", "support",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactoryCycleMaterialized {
    pub id: String,
    pub product_id: String,
    pub plan_id: String,
    pub created: bool,
}

pub fn materialize_factory_stage_task_graph(
    tx: &Transaction<'_>,
    owner_subject: &str,
    cycle_id: &str,
    stage: &str,
) -> Result<Option<String>> {
    let existing: Option<Option<String>> = tx.query_row(
        "SELECT run.task_graph_id FROM cp_factory_stage_runs run JOIN cp_factory_cycles cycle ON cycle.id=run.cycle_id JOIN cp_factory_products product ON product.id=cycle.product_id WHERE run.cycle_id=?1 AND run.stage=?2 AND product.owner_subject=?3",
        params![cycle_id, stage, owner_subject], |row| row.get(0),
    ).optional()?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    if let Some(graph_id) = existing {
        return Ok(Some(graph_id));
    }
    let graph_id = Uuid::new_v4().to_string();
    let task_id = Uuid::new_v4().to_string();
    let run_id = Uuid::new_v4().to_string();
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let title = format!("Factory {stage} stage");
    tx.execute("INSERT INTO task_graphs (id, goal, agent, status, created_at) VALUES (?1,?2,'factory','pending',?3)", params![graph_id, title, now])?;
    tx.execute("INSERT INTO cp_task_graphs (graph_id, goal, agent, created_at) VALUES (?1,?2,'factory',?3)", params![graph_id, title, now])?;
    tx.execute("INSERT INTO cp_tasks (id, plan_id, title, description, priority, status, assignee_kind, assignee_id, acceptance_criteria, created_at, updated_at) VALUES (?1,?2,?3,?4,40,'ready','agent','factory','Explicit operator approval is required before execution',?5,?5)", params![task_id, graph_id, title, "Factory lifecycle stage task graph; execution disabled by default", now])?;
    let run_contract_id =
        super::cp_insert_run_contract(tx, Some(&task_id), "", "[]", "[]", "[]", None, Some(600))?;
    tx.execute("INSERT INTO cp_agent_runs (id, task_id, provider, agent_name, run_contract_id, attempt, status, summary) VALUES (?1,?2,'factory','factory',?3,1,'pending','No automatic execution')", params![run_id, task_id, run_contract_id])?;
    tx.execute("INSERT INTO cp_task_graph_nodes (graph_id,node_id,task_id,agent_run_id,shell_cmd,created_at) VALUES (?1,?2,?3,?4,'',?5)", params![graph_id, stage, task_id, run_id, now])?;
    tx.execute(
        "INSERT INTO cp_approvals (id, task_id, agent_run_id, approval_type, reason, status, owner_subject, requested_at)
         VALUES (?1, ?2, ?3, 'factory_stage_execution', ?4, 'pending', ?5, ?6)",
        params![Uuid::new_v4().to_string(), task_id, run_id, format!("Approve Factory {stage} stage execution plan"), owner_subject, now],
    )?;
    tx.execute(
        "UPDATE cp_factory_stage_runs SET task_graph_id=?1 WHERE cycle_id=?2 AND stage=?3",
        params![graph_id, cycle_id, stage],
    )?;
    Ok(Some(graph_id))
}

pub fn activate_approved_factory_stage(
    tx: &Transaction<'_>,
    owner_subject: &str,
    cycle_id: &str,
    stage: &str,
) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_factory_stage_runs SET status='active', started_at=datetime('now','utc')
         WHERE cycle_id=?1 AND stage=?2 AND status='pending' AND task_graph_id IS NOT NULL
         AND EXISTS (
           SELECT 1 FROM cp_factory_cycles cycle
           JOIN cp_factory_products product ON product.id=cycle.product_id
           JOIN cp_task_graph_nodes node ON node.graph_id=cp_factory_stage_runs.task_graph_id
           JOIN cp_approvals approval ON approval.task_id=node.task_id
           WHERE cycle.id=cp_factory_stage_runs.cycle_id AND cycle.status IN ('planned','active') AND node.node_id=cp_factory_stage_runs.stage
             AND product.owner_subject=?3 AND approval.approval_type='factory_stage_execution'
             AND approval.status='approved' AND approval.owner_subject=?3
         )",
        params![cycle_id, stage, owner_subject],
    )?;
    if changed == 1 {
        tx.execute(
            "UPDATE cp_factory_cycles SET status='active', current_stage=?1 WHERE id=?2",
            params![stage, cycle_id],
        )?;
    }
    Ok(changed == 1)
}

pub fn complete_factory_stage_with_evidence(
    tx: &Transaction<'_>,
    owner: &str,
    cycle_id: &str,
    stage: &str,
) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_factory_stage_runs SET status='completed', completed_at=datetime('now','utc')
         WHERE cycle_id=?1 AND stage=?2 AND status='active'
           AND EXISTS (SELECT 1 FROM cp_factory_evidence_links evidence WHERE evidence.subject_kind='stage_run' AND evidence.subject_id=cp_factory_stage_runs.id AND evidence.content_ref IS NOT NULL AND trim(evidence.content_ref)<>'')
           AND EXISTS (SELECT 1 FROM cp_factory_cycles cycle JOIN cp_factory_products product ON product.id=cycle.product_id WHERE cycle.id=cp_factory_stage_runs.cycle_id AND cycle.status='active' AND product.owner_subject=?3)",
        params![cycle_id, stage, owner],
    )?;
    Ok(changed == 1)
}

/// Pause prevents new stage activation and evidence-backed completion until the
/// owner resumes the cycle. It never rewrites the active stage or run history.
pub fn pause_factory_cycle(tx: &Transaction<'_>, owner: &str, cycle_id: &str) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_factory_cycles SET status='paused'
         WHERE id=?1 AND status IN ('planned','active')
           AND EXISTS (SELECT 1 FROM cp_factory_products product WHERE product.id=cp_factory_cycles.product_id AND product.owner_subject=?2)",
        params![cycle_id, owner],
    )?;
    Ok(changed == 1)
}

/// Resume returns a paused cycle to `active` only if it already has an active
/// stage; otherwise it returns to `planned` so the next stage still needs its
/// normal approval and activation path.
pub fn resume_factory_cycle(tx: &Transaction<'_>, owner: &str, cycle_id: &str) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE cp_factory_cycles
         SET status=CASE WHEN EXISTS(SELECT 1 FROM cp_factory_stage_runs run WHERE run.cycle_id=cp_factory_cycles.id AND run.status='active') THEN 'active' ELSE 'planned' END
         WHERE id=?1 AND status='paused'
           AND EXISTS (SELECT 1 FROM cp_factory_products product WHERE product.id=cp_factory_cycles.product_id AND product.owner_subject=?2)",
        params![cycle_id, owner],
    )?;
    Ok(changed == 1)
}

/// Cancellation is terminal for the current cycle. Pending or active stage
/// rows are marked cancelled, preserving completed evidence and run records.
pub fn cancel_factory_cycle(tx: &Transaction<'_>, owner: &str, cycle_id: &str) -> Result<bool> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_cycles cycle JOIN cp_factory_products product ON product.id=cycle.product_id WHERE cycle.id=?1 AND cycle.status IN ('planned','active','paused') AND product.owner_subject=?2)",
        params![cycle_id, owner],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(false);
    }
    tx.execute(
        "UPDATE cp_factory_cycles SET status='cancelled' WHERE id=?1",
        [cycle_id],
    )?;
    tx.execute(
        "UPDATE cp_factory_stage_runs SET status='cancelled'
         WHERE cycle_id=?1 AND status IN ('pending','active')",
        [cycle_id],
    )?;
    Ok(true)
}

pub fn materialize_factory_cycle(
    tx: &Transaction<'_>,
    owner_subject: &str,
    plan_id: &str,
) -> Result<Option<FactoryCycleMaterialized>> {
    let plan: Option<String> = tx
        .query_row(
            "SELECT plan.product_id FROM cp_plans plan
             JOIN cp_factory_products product ON product.id = plan.product_id
             WHERE plan.id = ?1 AND plan.status = 'approved' AND product.owner_subject = ?2",
            params![plan_id, owner_subject],
            |row| row.get(0),
        )
        .optional()?;
    let Some(product_id) = plan else {
        return Ok(None);
    };
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM cp_factory_cycles WHERE plan_id = ?1 ORDER BY created_at ASC LIMIT 1",
            [plan_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(Some(FactoryCycleMaterialized {
            id,
            product_id,
            plan_id: plan_id.into(),
            created: false,
        }));
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_cycles (id, product_id, plan_id, status, current_stage)
         VALUES (?1, ?2, ?3, 'planned', ?4)",
        params![id, product_id, plan_id, FACTORY_LIFECYCLE_STAGES[0]],
    )?;
    for stage in FACTORY_LIFECYCLE_STAGES {
        tx.execute(
            "INSERT INTO cp_factory_stage_runs (id, cycle_id, stage, status) VALUES (?1, ?2, ?3, 'pending')",
            params![Uuid::new_v4().to_string(), id, stage],
        )?;
    }
    Ok(Some(FactoryCycleMaterialized {
        id,
        product_id,
        plan_id: plan_id.into(),
        created: true,
    }))
}

pub fn submit_factory_change_request(
    tx: &Transaction<'_>,
    owner_subject: &str,
    product_id: &str,
    summary: &str,
) -> Result<Option<String>> {
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cp_factory_products WHERE id = ?1 AND owner_subject = ?2)",
        params![product_id, owner_subject],
        |row| row.get(0),
    )?;
    if !owned {
        return Ok(None);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_change_requests (id, product_id, requested_by, status, summary)
         VALUES (?1, ?2, ?3, 'proposed', ?4)",
        params![id, product_id, owner_subject, summary],
    )?;
    Ok(Some(id))
}

pub fn assess_factory_change_request(
    tx: &Transaction<'_>,
    owner_subject: &str,
    change_request_id: &str,
) -> Result<Option<(String, u32)>> {
    let product_id: Option<String> = tx
        .query_row(
            "SELECT change.product_id FROM cp_factory_change_requests change
         JOIN cp_factory_products product ON product.id = change.product_id
         WHERE change.id = ?1 AND product.owner_subject = ?2 AND change.status = 'proposed'",
            params![change_request_id, owner_subject],
            |row| row.get(0),
        )
        .optional()?;
    let Some(product_id) = product_id else {
        return Ok(None);
    };
    let assessment_id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO cp_factory_impact_assessments (id, change_request_id, status, affected_count)
         VALUES (?1, ?2, 'ready', 0)",
        params![assessment_id, change_request_id],
    )?;
    let mut affected = 0u32;
    for (kind, query) in [
        (
            "requirement",
            "SELECT id FROM cp_factory_requirements WHERE product_id = ?1",
        ),
        (
            "charter_revision",
            "SELECT id FROM cp_factory_charter_revisions WHERE product_id = ?1",
        ),
    ] {
        let mut statement = tx.prepare(query)?;
        let ids = statement.query_map([&product_id], |row| row.get::<_, String>(0))?;
        for id in ids.flatten() {
            tx.execute(
                "INSERT INTO cp_factory_impact_targets (id, assessment_id, target_kind, target_id, relation_kind, staleness_state)
                 VALUES (?1, ?2, ?3, ?4, 'affected_by', 'stale')",
                params![Uuid::new_v4().to_string(), assessment_id, kind, id],
            )?;
            affected = affected.saturating_add(1);
        }
    }
    tx.execute(
        "UPDATE cp_factory_impact_assessments SET affected_count = ?1 WHERE id = ?2",
        params![affected, assessment_id],
    )?;
    tx.execute(
        "UPDATE cp_factory_change_requests SET status = 'awaiting_approval' WHERE id = ?1",
        [change_request_id],
    )?;
    Ok(Some((assessment_id, affected)))
}

pub fn resolve_factory_impact_assessment(
    tx: &Transaction<'_>,
    owner: &str,
    id: &str,
    approved: bool,
) -> Result<bool> {
    let change: Option<String> = tx.query_row("SELECT change.id FROM cp_factory_impact_assessments impact JOIN cp_factory_change_requests change ON change.id=impact.change_request_id JOIN cp_factory_products product ON product.id=change.product_id WHERE impact.id=?1 AND product.owner_subject=?2 AND impact.status='ready' AND change.status='awaiting_approval'", params![id, owner], |row| row.get(0)).optional()?;
    let Some(change) = change else {
        return Ok(false);
    };
    let status = if approved { "accepted" } else { "rejected" };
    tx.execute("UPDATE cp_factory_impact_assessments SET status=?1, accepted_at=datetime('now','utc') WHERE id=?2", params![status, id])?;
    tx.execute("UPDATE cp_factory_change_requests SET status=?1, resolved_at=datetime('now','utc') WHERE id=?2", params![status, change])?;
    Ok(true)
}
