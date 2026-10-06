//! Agent launch validation shared by `LaunchAgent` (the daemon then opens a terminal) and
//! `ValidateAgentLaunch` (the client runs the agent itself; the daemon starts nothing).

use raios_contracts::Problem;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::service::{
    validate_agent_name, validate_agent_prompt, validate_project_path, AgentLaunchRequest,
};

/// Checks a launch request: known agent, absolute path of a registered project, bounded prompt
/// without credentials. The checks run in this order so both commands report the same error for
/// the same bad input.
pub(super) fn validate_launch(
    conn: &Connection,
    agent_name: &str,
    project_path: &str,
    prompt: Option<&str>,
) -> Result<AgentLaunchRequest, Problem> {
    let agent_name = validate_agent_name(agent_name)?;
    let project_path = validate_project_path(Some(project_path))?
        .ok_or_else(|| Problem::invalid_input("Project path is required"))?;
    let project_exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE path = ?1",
            params![project_path],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| Problem::internal(error.to_string()))?;
    if project_exists.is_none() {
        return Err(Problem::not_found(
            "Agent launch requires a registered project path",
        ));
    }
    let prompt = validate_agent_prompt(prompt)?;
    Ok(AgentLaunchRequest {
        agent_name,
        project_path,
        prompt,
    })
}

/// The JSON result both commands report, with the normalized agent name and a `status`.
pub(super) fn launch_result(request: &AgentLaunchRequest, status: &str) -> Value {
    json!({
        "agent_name": request.agent_name,
        "project_path": request.project_path,
        "status": status,
    })
}

#[cfg(test)]
mod tests;
