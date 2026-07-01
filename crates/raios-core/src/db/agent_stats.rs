//! Per-agent performance aggregation over `cp_agent_runs`.
//!
//! Scope, honestly stated: this reports what the schema actually tracks
//! today — run counts, success rate, wall-clock duration, and exit_reason
//! distribution. Token usage and repetition (how much an agent re-does the
//! same work) are not persisted anywhere in `cp_agent_runs` or the budget
//! ledger (`cp_budget_ledger` tracks *limits*, not per-run consumption), so
//! this module does not report them — no fabricated numbers.

use rusqlite::{Connection, Result};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AgentStats {
    pub agent_name: String,
    pub total_runs: i64,
    pub succeeded: i64,
    pub failed: i64,
    /// Runs still `running`/`in_progress` — e.g. an interactive wrapper
    /// session whose process ended without going through `cp_session_end`
    /// (killed terminal, crashed CLI). Excluded from `success_rate`'s
    /// denominator so an agent isn't penalized for sessions that never
    /// actually concluded one way or the other.
    pub still_open: i64,
    /// `succeeded / (succeeded + failed)` — terminal runs only. `None` if no
    /// run has concluded yet.
    pub success_rate: Option<f64>,
    /// Average wall-clock duration in seconds across runs that have both a
    /// parseable `started_at` and `ended_at`. `None` if none qualify.
    pub avg_duration_secs: Option<f64>,
    /// `(exit_reason, count)`, most frequent first. Runs with no
    /// `exit_reason` (still running, or never set) are excluded.
    pub top_exit_reasons: Vec<(String, i64)>,
}

const TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S";

struct RunRow {
    status: String,
    exit_reason: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
}

/// Aggregates `cp_agent_runs` into one `AgentStats` per `agent_name`, sorted
/// by `agent_name`.
pub fn cp_agent_stats(conn: &Connection) -> Result<Vec<AgentStats>> {
    let mut stmt = conn.prepare(
        "SELECT agent_name, status, exit_reason, started_at, ended_at \
         FROM cp_agent_runs WHERE agent_name IS NOT NULL AND agent_name != ''",
    )?;
    let mut by_agent: BTreeMap<String, Vec<RunRow>> = BTreeMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            RunRow {
                status: row.get(1)?,
                exit_reason: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
            },
        ))
    })?;
    for r in rows {
        let (agent_name, run) = r?;
        by_agent.entry(agent_name).or_default().push(run);
    }

    Ok(by_agent
        .into_iter()
        .map(|(agent_name, runs)| summarize(agent_name, runs))
        .collect())
}

/// Same aggregation, scoped to a single agent. `None` if that agent has no
/// runs at all (distinct from "has runs but all zero" — callers can tell
/// "never ran" from "ran and stats are just empty").
pub fn cp_agent_stats_for(conn: &Connection, agent_name: &str) -> Result<Option<AgentStats>> {
    Ok(cp_agent_stats(conn)?.into_iter().find(|s| s.agent_name == agent_name))
}

fn summarize(agent_name: String, runs: Vec<RunRow>) -> AgentStats {
    let total_runs = runs.len() as i64;
    let succeeded = runs.iter().filter(|r| r.status == "succeeded" || r.status == "completed").count() as i64;
    let failed = runs.iter().filter(|r| r.status == "failed").count() as i64;
    let still_open = total_runs - succeeded - failed;
    let terminal = succeeded + failed;
    let success_rate = (terminal > 0).then(|| succeeded as f64 / terminal as f64);

    let durations: Vec<f64> = runs
        .iter()
        .filter_map(|r| {
            let start = parse_ts(r.started_at.as_deref()?)?;
            let end = parse_ts(r.ended_at.as_deref()?)?;
            let secs = (end - start).num_seconds();
            (secs >= 0).then_some(secs as f64)
        })
        .collect();
    let avg_duration_secs =
        (!durations.is_empty()).then(|| durations.iter().sum::<f64>() / durations.len() as f64);

    let mut reason_counts: BTreeMap<String, i64> = BTreeMap::new();
    for r in &runs {
        if let Some(reason) = &r.exit_reason {
            *reason_counts.entry(reason.clone()).or_insert(0) += 1;
        }
    }
    let mut top_exit_reasons: Vec<(String, i64)> = reason_counts.into_iter().collect();
    top_exit_reasons.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    AgentStats {
        agent_name,
        total_runs,
        succeeded,
        failed,
        still_open,
        success_rate,
        avg_duration_secs,
        top_exit_reasons,
    }
}

fn parse_ts(s: &str) -> Option<chrono::NaiveDateTime> {
    chrono::NaiveDateTime::parse_from_str(s, TIMESTAMP_FMT).ok()
}
