use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};
pub enum BudgetGate {
    /// Proceed normally.
    Allow,
    /// Budget data is unreliable — allow but log.
    AllowUnknown,
    /// Budget is strained; defer optional work.
    SoftDefer,
    /// Budget is exhausted; do not start this work.
    HardBlock { metric: String, scope: String },
}

impl BudgetGate {
    pub fn is_blocked(&self) -> bool {
        matches!(self, BudgetGate::HardBlock { .. })
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cp_upsert_budget_ledger(
    conn: &Connection,
    scope_kind: &str,
    scope_id: &str,
    provider: Option<&str>,
    metric: &str,
    limit_value: Option<f64>,
    used_value: Option<f64>,
    remaining_value: Option<f64>,
    confidence: &str,
    source: &str,
) -> Result<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO cp_budget_ledger
         (id, scope_kind, scope_id, provider, metric, limit_value, used_value,
          remaining_value, confidence, source, observed_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id, scope_kind, scope_id, provider, metric,
            limit_value, used_value, remaining_value,
            confidence, source, now
        ],
    )?;
    Ok(())
}

/// Check whether a provider-level budget gate allows a task to proceed.
/// Only hard-blocks when remaining_value == 0 AND confidence is 'exact' or 'estimated'.
pub fn cp_check_provider_budget_gate(conn: &Connection, provider: &str) -> Result<BudgetGate> {
    // Check most recent ledger row for this provider with metric = 'tokens'
    let row: Option<(Option<f64>, Option<f64>, String)> = conn
        .query_row(
            "SELECT remaining_value, limit_value, confidence
             FROM cp_budget_ledger
             WHERE scope_kind = 'provider' AND scope_id = ?1 AND metric = 'tokens'
             ORDER BY observed_at DESC LIMIT 1",
            params![provider],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;

    match row {
        None => Ok(BudgetGate::AllowUnknown),
        Some((_, _, confidence)) if confidence == "unavailable" => Ok(BudgetGate::AllowUnknown),
        Some((Some(remaining), Some(_limit), confidence))
            if remaining <= 0.0 && (confidence == "exact" || confidence == "estimated") =>
        {
            Ok(BudgetGate::HardBlock {
                metric: "tokens".into(),
                scope: format!("provider:{}", provider),
            })
        }
        Some((Some(remaining), Some(limit_val), _)) if limit_val > 0.0 && remaining / limit_val < 0.1 => {
            Ok(BudgetGate::SoftDefer)
        }
        _ => Ok(BudgetGate::Allow),
    }
}

/// Check run-contract budget gates: token_budget and time_budget from the contract.
pub fn cp_check_contract_budget_gate(conn: &Connection, task_id: &str) -> Result<BudgetGate> {
    let contract = cp_get_run_contract_for_agent_run(conn, task_id)?;
    match contract {
        None => Ok(BudgetGate::AllowUnknown),
        Some(c) => {
            // If contract specifies a token_budget of 0, block immediately
            if c.token_budget == Some(0) {
                return Ok(BudgetGate::HardBlock {
                    metric: "tokens".into(),
                    scope: format!("contract:{}", c.id),
                });
            }
            Ok(BudgetGate::Allow)
        }
    }
}

// ── Phase 6: Provider normalization ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub provider: String,
    pub supports_tool_calling: bool,
    pub supports_patch_diff: bool,
    pub supports_long_running: bool,
    pub supports_streaming: bool,
    pub supports_exact_quota_visibility: bool,
}

impl ProviderCapabilities {
    pub fn seed_known() -> Vec<Self> {
        vec![
            Self {
                provider: "claude".into(),
                supports_tool_calling: true,
                supports_patch_diff: true,
                supports_long_running: true,
                supports_streaming: true,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "codex".into(),
                supports_tool_calling: false,
                supports_patch_diff: true,
                supports_long_running: false,
                supports_streaming: false,
                supports_exact_quota_visibility: true,
            },
            Self {
                provider: "swarm".into(),
                supports_tool_calling: true,
                supports_patch_diff: true,
                supports_long_running: true,
                supports_streaming: false,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "opencode".into(),
                supports_tool_calling: true,
                supports_patch_diff: true,
                supports_long_running: true,
                supports_streaming: true,
                supports_exact_quota_visibility: false,
            },
            Self {
                provider: "shell".into(),
                supports_tool_calling: false,
                supports_patch_diff: false,
                supports_long_running: true,
                supports_streaming: true,
                supports_exact_quota_visibility: true,
            },
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Auth,
    Quota,
    Timeout,
    Sandbox,
    ToolError,
    HumanRejection,
    ProviderUnavailable,
    Unknown(String),
}

impl ProviderFailureKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Auth => "auth",
            Self::Quota => "quota",
            Self::Timeout => "timeout",
            Self::Sandbox => "sandbox",
            Self::ToolError => "tool_error",
            Self::HumanRejection => "human_rejection",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn from_stored(s: &str) -> Self {
        match s {
            "auth" => Self::Auth,
            "quota" => Self::Quota,
            "timeout" => Self::Timeout,
            "sandbox" => Self::Sandbox,
            "tool_error" => Self::ToolError,
            "human_rejection" => Self::HumanRejection,
            "provider_unavailable" => Self::ProviderUnavailable,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn classify(error: &str) -> Self {
        let lower = error.to_lowercase();
        if lower.contains("auth")
            || lower.contains("401")
            || lower.contains("403")
            || lower.contains("unauthorized")
            || lower.contains("api key")
        {
            Self::Auth
        } else if lower.contains("quota")
            || lower.contains("rate limit")
            || lower.contains("429")
            || lower.contains("token limit")
            || lower.contains("context length")
        {
            Self::Quota
        } else if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline") {
            Self::Timeout
        } else if lower.contains("sandbox")
            || lower.contains("permission denied")
            || lower.contains("access denied")
        {
            Self::Sandbox
        } else if lower.contains("tool")
            || lower.contains("function call")
            || lower.contains("invalid argument")
        {
            Self::ToolError
        } else if lower.contains("rejected") || lower.contains("declined") || lower.contains("human") {
            Self::HumanRejection
        } else if lower.contains("unavailable")
            || lower.contains("503")
            || lower.contains("connection refused")
            || lower.contains("network")
        {
            Self::ProviderUnavailable
        } else {
            Self::Unknown(error.to_string())
        }
    }
}

pub fn cp_upsert_provider_capabilities(conn: &Connection, caps: &ProviderCapabilities) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO cp_provider_capabilities
            (provider, supports_tool_calling, supports_patch_diff, supports_long_running,
             supports_streaming, supports_exact_quota_visibility, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(provider) DO UPDATE SET
            supports_tool_calling          = excluded.supports_tool_calling,
            supports_patch_diff            = excluded.supports_patch_diff,
            supports_long_running          = excluded.supports_long_running,
            supports_streaming             = excluded.supports_streaming,
            supports_exact_quota_visibility = excluded.supports_exact_quota_visibility,
            updated_at                     = excluded.updated_at",
        params![
            caps.provider,
            caps.supports_tool_calling as i64,
            caps.supports_patch_diff as i64,
            caps.supports_long_running as i64,
            caps.supports_streaming as i64,
            caps.supports_exact_quota_visibility as i64,
            now,
        ],
    )?;
    Ok(())
}

pub fn cp_get_provider_capabilities(
    conn: &Connection,
    provider: &str,
) -> Result<Option<ProviderCapabilities>> {
    conn.query_row(
        "SELECT provider, supports_tool_calling, supports_patch_diff, supports_long_running,
                supports_streaming, supports_exact_quota_visibility
         FROM cp_provider_capabilities WHERE provider = ?1",
        params![provider],
        |r| {
            Ok(ProviderCapabilities {
                provider: r.get(0)?,
                supports_tool_calling: r.get::<_, i64>(1)? != 0,
                supports_patch_diff: r.get::<_, i64>(2)? != 0,
                supports_long_running: r.get::<_, i64>(3)? != 0,
                supports_streaming: r.get::<_, i64>(4)? != 0,
                supports_exact_quota_visibility: r.get::<_, i64>(5)? != 0,
            })
        },
    )
    .optional()
}

pub fn cp_list_provider_capabilities(conn: &Connection) -> Result<Vec<ProviderCapabilities>> {
    let mut stmt = conn.prepare(
        "SELECT provider, supports_tool_calling, supports_patch_diff, supports_long_running,
                supports_streaming, supports_exact_quota_visibility
         FROM cp_provider_capabilities ORDER BY provider",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProviderCapabilities {
                provider: r.get(0)?,
                supports_tool_calling: r.get::<_, i64>(1)? != 0,
                supports_patch_diff: r.get::<_, i64>(2)? != 0,
                supports_long_running: r.get::<_, i64>(3)? != 0,
                supports_streaming: r.get::<_, i64>(4)? != 0,
                supports_exact_quota_visibility: r.get::<_, i64>(5)? != 0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Seed known provider capabilities. Skips providers already registered (no overwrites).
pub fn cp_seed_provider_capabilities(conn: &Connection) -> Result<()> {
    for caps in ProviderCapabilities::seed_known() {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cp_provider_capabilities WHERE provider = ?1",
            params![caps.provider],
            |r| r.get(0),
        )?;
        if exists == 0 {
            cp_upsert_provider_capabilities(conn, &caps)?;
        }
    }
    Ok(())
}

/// Record a normalized failure on an agent run and transition its parent task to 'failed'.
pub fn cp_record_run_failure(
    conn: &Connection,
    agent_run_id: &str,
    kind: &ProviderFailureKind,
    detail: &str,
) -> Result<()> {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "UPDATE cp_agent_runs SET status='failed', ended_at=?1, exit_reason=?2 WHERE id=?3",
        params![now, kind.as_str(), agent_run_id],
    )?;
    conn.execute(
        "UPDATE cp_tasks SET status='failed', updated_at=?1
         WHERE id = (SELECT task_id FROM cp_agent_runs WHERE id = ?2)
           AND status NOT IN ('completed','cancelled')",
        params![now, agent_run_id],
    )?;
    if !detail.is_empty() {
        let label = kind.as_str();
        conn.execute(
            "UPDATE cp_agent_runs
             SET summary = COALESCE(summary || ' | ', '') || ?1
             WHERE id = ?2",
            params![format!("[{label}] {detail}"), agent_run_id],
        )?;
    }
    Ok(())
}

/// Returns true if the provider supports all specified capabilities.
/// Permissive when provider has no registered row.
pub fn cp_check_provider_supports(
    conn: &Connection,
    provider: &str,
    needs_tool_calling: bool,
    needs_patch_diff: bool,
    needs_long_running: bool,
) -> Result<bool> {
    match cp_get_provider_capabilities(conn, provider)? {
        None => Ok(true),
        Some(c) => Ok((!needs_tool_calling || c.supports_tool_calling)
            && (!needs_patch_diff || c.supports_patch_diff)
            && (!needs_long_running || c.supports_long_running)),
    }
}

/// Return the first provider from the preference list that satisfies all capability requirements.
pub fn cp_route_to_capable_provider(
    conn: &Connection,
    needs_tool_calling: bool,
    needs_patch_diff: bool,
    needs_long_running: bool,
) -> Result<Option<String>> {
    let providers = cp_list_provider_capabilities(conn)?;
    let preferred = ["claude", "codex", "opencode", "swarm", "shell"];
    for name in &preferred {
        if let Some(p) = providers.iter().find(|p| p.provider == *name) {
            if (!needs_tool_calling || p.supports_tool_calling)
                && (!needs_patch_diff || p.supports_patch_diff)
                && (!needs_long_running || p.supports_long_running)
            {
                return Ok(Some(p.provider.clone()));
            }
        }
    }
    for p in &providers {
        if (!needs_tool_calling || p.supports_tool_calling)
            && (!needs_patch_diff || p.supports_patch_diff)
            && (!needs_long_running || p.supports_long_running)
        {
            return Ok(Some(p.provider.clone()));
        }
    }
    Ok(None)
}

// ── Run contracts ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunContract {
    pub id: String,
    pub task_id: Option<String>,
    pub workspace_root: String,
    pub allowed_paths_json: String,
    pub blocked_paths_json: String,
    pub allowed_tools_json: String,
    pub network_policy_json: String,
    pub token_budget: Option<i64>,
    pub time_budget_secs: Option<i64>,
    pub cpu_budget_pct: Option<f64>,
    pub memory_budget_mb: Option<i64>,
    pub expected_artifacts_json: String,
    pub success_criteria_json: String,
    pub escalation_policy_json: String,
    pub created_at: String,
}

pub struct RunContractBuilder {
    task_id: Option<String>,
    workspace_root: String,
    allowed_paths: Vec<String>,
    blocked_paths: Vec<String>,
    allowed_tools: Vec<String>,
    token_budget: Option<i64>,
    time_budget_secs: Option<i64>,
}

impl RunContractBuilder {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self {
            task_id: None,
            workspace_root: workspace_root.into(),
            allowed_paths: vec![],
            blocked_paths: vec![],
            allowed_tools: vec![],
            token_budget: None,
            time_budget_secs: None,
        }
    }

    pub fn task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = Some(id.into());
        self
    }

    pub fn allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = paths;
        self
    }

    pub fn blocked_paths(mut self, paths: Vec<String>) -> Self {
        self.blocked_paths = paths;
        self
    }

    pub fn allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    pub fn token_budget(mut self, tokens: i64) -> Self {
        self.token_budget = Some(tokens);
        self
    }

    pub fn time_budget_secs(mut self, secs: i64) -> Self {
        self.time_budget_secs = Some(secs);
        self
    }

    pub fn insert(self, conn: &Connection) -> Result<String> {
        cp_insert_run_contract(
            conn,
            self.task_id.as_deref(),
            &self.workspace_root,
            &serde_json::to_string(&self.allowed_paths).unwrap_or_else(|_| "[]".into()),
            &serde_json::to_string(&self.blocked_paths).unwrap_or_else(|_| "[]".into()),
            &serde_json::to_string(&self.allowed_tools).unwrap_or_else(|_| "[]".into()),
            self.token_budget,
            self.time_budget_secs,
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub fn cp_insert_run_contract(
    conn: &Connection,
    task_id: Option<&str>,
    workspace_root: &str,
    allowed_paths_json: &str,
    blocked_paths_json: &str,
    allowed_tools_json: &str,
    token_budget: Option<i64>,
    time_budget_secs: Option<i64>,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    conn.execute(
        "INSERT INTO cp_run_contracts
         (id, task_id, workspace_root, allowed_paths_json, blocked_paths_json,
          allowed_tools_json, token_budget, time_budget_secs, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            task_id,
            workspace_root,
            allowed_paths_json,
            blocked_paths_json,
            allowed_tools_json,
            token_budget,
            time_budget_secs,
            now
        ],
    )?;
    Ok(id)
}

pub fn cp_get_run_contract(conn: &Connection, id: &str) -> Result<Option<RunContract>> {
    conn.query_row(
        "SELECT id, task_id, workspace_root, allowed_paths_json, blocked_paths_json,
                allowed_tools_json, network_policy_json, token_budget, time_budget_secs,
                cpu_budget_pct, memory_budget_mb, expected_artifacts_json,
                success_criteria_json, escalation_policy_json, created_at
         FROM cp_run_contracts WHERE id = ?1",
        params![id],
        |r| {
            Ok(RunContract {
                id: r.get(0)?,
                task_id: r.get(1)?,
                workspace_root: r.get(2)?,
                allowed_paths_json: r.get(3)?,
                blocked_paths_json: r.get(4)?,
                allowed_tools_json: r.get(5)?,
                network_policy_json: r.get(6)?,
                token_budget: r.get(7)?,
                time_budget_secs: r.get(8)?,
                cpu_budget_pct: r.get(9)?,
                memory_budget_mb: r.get(10)?,
                expected_artifacts_json: r.get(11)?,
                success_criteria_json: r.get(12)?,
                escalation_policy_json: r.get(13)?,
                created_at: r.get(14)?,
            })
        },
    )
    .optional()
}

/// Returns the run contract for a given agent run, if the run_contract_id is a real UUID.
pub fn cp_get_run_contract_for_agent_run(
    conn: &Connection,
    agent_run_id: &str,
) -> Result<Option<RunContract>> {
    let contract_id: Option<String> = conn
        .query_row(
            "SELECT run_contract_id FROM cp_agent_runs WHERE id = ?1",
            params![agent_run_id],
            |r| r.get(0),
        )
        .optional()?;

    match contract_id {
        Some(cid) => cp_get_run_contract(conn, &cid),
        None => Ok(None),
    }
}
