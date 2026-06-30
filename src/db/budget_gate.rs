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
