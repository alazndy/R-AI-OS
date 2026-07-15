//! A2A (Agent2Agent) protocol surface — a minimal JSON-RPC 2.0 endpoint so
//! agents outside the Claude/Codex/OpenCode/Antigravity CLI matrix can
//! interoperate with R-AI-OS's existing handoff control plane instead of
//! only R-AI-OS's own four `*_kaira` identities being able to hand off work.
//!
//! Scope, honestly stated: this is not a full A2A implementation (no
//! streaming, no push notifications, no multi-turn task lifecycle beyond
//! what the existing handoff workflow already models). It maps the two A2A
//! operations that have a direct analog in the control plane:
//!   - `message/send`  → `create_handoff_workflow` (same as `raios handoff`)
//!   - `tasks/get` / `tasks/list` → the same `cp_query_active_tasks` read
//!     model already used by `GET /api/inbox` (`routes.rs::handle_inbox`)
//!
//! `GET /.well-known/agent.json` is intentionally left out of the Bearer-auth
//! gate (see `auth.rs`) — an Agent Card is a public discovery document by
//! convention, the same way OAuth/OIDC discovery endpoints are unauthenticated.
//! `POST /a2a` itself stays behind the same auth middleware as every other
//! `/api/*` route.

use axum::{response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "0.1";

/// GET /.well-known/agent.json — the A2A Agent Card.
pub(super) async fn handle_agent_card() -> impl IntoResponse {
    Json(json!({
        "name": "R-AI-OS",
        "description": "Hardened multi-agent orchestration kernel: security policy gate, tamper-evident audit ledger, and atomic agent handoff for Claude Code, Codex CLI, OpenCode, and Antigravity.",
        "url": "http://localhost:42071/a2a",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "streaming": false, "pushNotifications": false },
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": [
            {
                "id": "handoff",
                "name": "Agent Handoff",
                "description": "Hand a task off to one of R-AI-OS's agent identities via message/send; delivered as [HANDOVER CONTEXT] on that agent's next run."
            },
            {
                "id": "task_status",
                "name": "Task Status",
                "description": "Query active tasks via tasks/get or tasks/list."
            }
        ],
        "agents": ["claude_kaira", "codex_kaira", "opencode_kaira", "antigravity_kaira"]
    }))
}

#[derive(Debug, Deserialize)]
pub(super) struct A2aRequest {
    /// Same as mcp::RpcRequest.jsonrpc — must deserialize, value unused.
    #[allow(dead_code)]
    #[serde(default)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct A2aResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<A2aError>,
}

#[derive(Debug, Serialize)]
struct A2aError {
    code: i32,
    message: String,
}

impl A2aResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }
    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(A2aError { code, message: message.into() }) }
    }
}

/// POST /a2a — the JSON-RPC 2.0 dispatch entry point.
pub(super) async fn handle_a2a(Json(req): Json<A2aRequest>) -> impl IntoResponse {
    let id = req.id.clone().unwrap_or(Value::Null);
    let result = match req.method.as_str() {
        "message/send" => a2a_message_send(&req.params),
        "tasks/get" => a2a_tasks_get(&req.params),
        "tasks/list" => a2a_tasks_list(),
        other => Err(format!("method_not_found:unknown A2A method '{other}'")),
    };
    Json(match result {
        Ok(v) => A2aResponse::ok(id, v),
        Err(msg) if msg.starts_with("method_not_found:") => A2aResponse::err(id, -32601, msg),
        Err(msg) if msg.starts_with("invalid_params:") => A2aResponse::err(id, -32602, msg),
        Err(msg) if msg.starts_with("secret_detected:") => A2aResponse::err(id, -32010, msg),
        Err(msg) => A2aResponse::err(id, -32603, msg),
    })
}

/// `message/send`: creates a handoff exactly the way `raios handoff` does,
/// so the receiving agent picks it up via the existing
/// `cp_take_pending_handoff` → `[HANDOVER CONTEXT]` delivery path — no
/// separate A2A-specific delivery mechanism to keep in sync.
fn a2a_message_send(params: &Value) -> Result<Value, String> {
    let to_agent = params["to"]
        .as_str()
        .ok_or("invalid_params:missing 'to' agent identity")?;
    let text = extract_message_text(params).ok_or("invalid_params:missing message text")?;
    let project_path = params["project_path"].as_str().unwrap_or(".");

    if let Some(label) = raios_core::security::looks_like_secret(&text) {
        return Err(format!(
            "secret_detected:message looks like it contains a {label} — refused, resend without it"
        ));
    }

    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    let ids = raios_core::db::create_handoff_workflow(
        &conn,
        raios_core::db::HandoffWorkflowInput {
            project_path,
            from_agent: "a2a_remote",
            to_agent,
            status: "SUCCESS",
            msg: &text,
            diff_stat: None,
            report: None,
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(json!({
        "task": {
            "id": ids.task_id,
            "status": "submitted",
            "artifacts": [{ "id": ids.artifact_id, "kind": "handover_note" }]
        }
    }))
}

/// A2A messages carry text in `message.parts[].text`; also accept a bare
/// top-level `text` field for simpler callers that don't build the full
/// `Message`/`Part` structure.
fn extract_message_text(params: &Value) -> Option<String> {
    params["message"]["parts"]
        .as_array()
        .and_then(|parts| parts.iter().find_map(|p| p["text"].as_str()))
        .map(str::to_string)
        .or_else(|| params["text"].as_str().map(str::to_string))
}

fn a2a_tasks_get(params: &Value) -> Result<Value, String> {
    let task_id = params["id"].as_str().ok_or("invalid_params:missing task 'id'")?;
    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    let tasks = raios_core::db::cp_query_active_tasks(&conn).map_err(|e| e.to_string())?;
    tasks
        .into_iter()
        .find(|t| t.id == task_id)
        .map(|t| json!({ "id": t.id, "status": t.status, "title": t.title, "origin": t.origin }))
        .ok_or_else(|| format!("invalid_params:no active task with id '{task_id}'"))
}

fn a2a_tasks_list() -> Result<Value, String> {
    let conn = raios_core::db::open_db().map_err(|e| e.to_string())?;
    let tasks = raios_core::db::cp_query_active_tasks(&conn).map_err(|e| e.to_string())?;
    Ok(json!({ "tasks": tasks }))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_message_text_reads_a2a_parts_shape() {
        let params = json!({
            "to": "claude_kaira",
            "message": { "parts": [{ "type": "text", "text": "skeleton ready" }] }
        });
        assert_eq!(extract_message_text(&params).as_deref(), Some("skeleton ready"));
    }

    #[test]
    fn extract_message_text_falls_back_to_bare_text_field() {
        let params = json!({ "to": "claude_kaira", "text": "quick note" });
        assert_eq!(extract_message_text(&params).as_deref(), Some("quick note"));
    }

    #[test]
    fn extract_message_text_none_when_absent() {
        let params = json!({ "to": "claude_kaira" });
        assert_eq!(extract_message_text(&params), None);
    }

    #[test]
    fn message_send_rejects_missing_to_field() {
        let params = json!({ "text": "hello" });
        let err = a2a_message_send(&params).unwrap_err();
        assert!(err.starts_with("invalid_params:"));
    }

    #[test]
    fn message_send_rejects_secret_looking_text() {
        let params = json!({
            "to": "claude_kaira",
            "text": "here's the key sk-ant-abcdefghijklmnopqrstuvwxyz"
        });
        let err = a2a_message_send(&params).unwrap_err();
        assert!(err.starts_with("secret_detected:"), "got: {err}");
    }

    #[test]
    fn tasks_get_rejects_missing_id() {
        let err = a2a_tasks_get(&json!({})).unwrap_err();
        assert!(err.starts_with("invalid_params:"));
    }
}
