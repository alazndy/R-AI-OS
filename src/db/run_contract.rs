use rusqlite::{params, Connection, OptionalExtension, Result};

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
