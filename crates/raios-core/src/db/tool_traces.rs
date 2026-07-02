use rusqlite::{params, Connection, OptionalExtension, Result};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolTraceRow {
    pub id: String,
    pub project: String,
    pub agent: String,
    pub command: String,
    pub context: String,
    pub outcome: String,
    pub error_summary: String,
    pub fix_summary: String,
    pub tags_json: String,
    pub success: bool,
    pub confidence: f64,
    pub related_task_id: Option<String>,
    pub content_hash: String,
    pub redacted: bool,
    pub created_at: String,
}

pub struct ToolTraceInsert<'a> {
    pub project: &'a str,
    pub agent: &'a str,
    pub command: &'a str,
    pub context: &'a str,
    pub outcome: &'a str,
    pub error_summary: &'a str,
    pub fix_summary: &'a str,
    pub tags_json: &'a str,
    pub success: bool,
    pub confidence: f64,
    pub related_task_id: Option<&'a str>,
}

pub struct ToolTraceQuery<'a> {
    pub text: &'a str,
    pub project: Option<&'a str>,
    pub preferred_project: Option<&'a str>,
    pub success_only: bool,
    pub tag: Option<&'a str>,
    pub limit: usize,
}

pub fn tool_trace_insert(conn: &Connection, trace: ToolTraceInsert) -> Result<Option<String>> {
    let id = uuid::Uuid::new_v4().to_string();
    let hash = tool_trace_hash(&trace);
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO tool_traces
            (id, project, agent, command, context, outcome, error_summary, fix_summary,
             tags_json, success, confidence, related_task_id, content_hash, redacted, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0, ?14)",
        params![
            id,
            trace.project,
            trace.agent,
            trace.command,
            trace.context,
            trace.outcome,
            trace.error_summary,
            trace.fix_summary,
            trace.tags_json,
            trace.success as i32,
            trace.confidence,
            trace.related_task_id,
            hash,
            now
        ],
    )?;
    Ok((inserted > 0).then_some(id))
}

pub fn tool_trace_record_secret_refusal(
    conn: &Connection,
    project: &str,
    agent: &str,
    label: &str,
) -> Result<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let hash = hash_parts(&[project, agent, label, &id, "secret_refusal"]);
    conn.execute(
        "INSERT INTO tool_traces
            (id, project, agent, command, context, outcome, error_summary, fix_summary,
             tags_json, success, confidence, related_task_id, content_hash, redacted, created_at)
         VALUES (?1, ?2, ?3, '[refused: secret-like content]', '', ?4, '', '',
             '[]', 0, 1.0, NULL, ?5, 1, ?6)",
        params![
            id,
            project,
            agent,
            format!("refused_secret:{label}"),
            hash,
            now
        ],
    )?;
    Ok(id)
}

pub fn tool_trace_get(conn: &Connection, id: &str) -> Result<Option<ToolTraceRow>> {
    conn.query_row(
        "SELECT id, project, agent, command, context, outcome, error_summary, fix_summary,
                tags_json, success, confidence, related_task_id, content_hash, redacted, created_at
         FROM tool_traces WHERE id = ?1",
        params![id],
        row_from_sql,
    )
    .optional()
}

pub fn tool_trace_forget(conn: &Connection, id: &str) -> Result<bool> {
    let deleted = conn.execute("DELETE FROM tool_traces WHERE id = ?1", params![id])?;
    Ok(deleted > 0)
}

pub fn tool_trace_search(conn: &Connection, query: ToolTraceQuery) -> Result<Vec<ToolTraceRow>> {
    let limit = query.limit.clamp(1, 100);
    let like = format!("%{}%", query.text.trim());
    let project_like = query.project.map(|p| format!("%{p}%"));
    let tag_like = query.tag.map(|t| format!("%{t}%"));
    let preferred_project_like = query.preferred_project.map(|p| format!("%{p}%"));
    let mut stmt = conn.prepare(
        "SELECT id, project, agent, command, context, outcome, error_summary, fix_summary,
                tags_json, success, confidence, related_task_id, content_hash, redacted, created_at
         FROM tool_traces
         WHERE redacted = 0
           AND (?1 = '' OR command LIKE ?2 OR context LIKE ?2 OR outcome LIKE ?2
                OR error_summary LIKE ?2 OR fix_summary LIKE ?2 OR tags_json LIKE ?2)
           AND (?3 IS NULL OR project LIKE ?3)
           AND (?4 = 0 OR success = 1)
           AND (?5 IS NULL OR tags_json LIKE ?5)
         ORDER BY
           CASE WHEN ?6 IS NOT NULL AND project LIKE ?6 THEN 25 ELSE 0 END +
           CASE WHEN success = 1 THEN 10 ELSE 0 END +
           confidence * 10 DESC,
           created_at DESC
         LIMIT ?7",
    )?;
    let rows = stmt
        .query_map(
            params![
                query.text.trim(),
                like,
                project_like.as_deref(),
                query.success_only as i32,
                tag_like.as_deref(),
                preferred_project_like.as_deref(),
                limit as i64
            ],
            row_from_sql,
        )?
        .flatten()
        .collect();
    Ok(rows)
}

fn row_from_sql(row: &rusqlite::Row<'_>) -> Result<ToolTraceRow> {
    Ok(ToolTraceRow {
        id: row.get(0)?,
        project: row.get(1)?,
        agent: row.get(2)?,
        command: row.get(3)?,
        context: row.get(4)?,
        outcome: row.get(5)?,
        error_summary: row.get(6)?,
        fix_summary: row.get(7)?,
        tags_json: row.get(8)?,
        success: row.get::<_, i64>(9)? != 0,
        confidence: row.get(10)?,
        related_task_id: row.get(11)?,
        content_hash: row.get(12)?,
        redacted: row.get::<_, i64>(13)? != 0,
        created_at: row.get(14)?,
    })
}

fn tool_trace_hash(trace: &ToolTraceInsert<'_>) -> String {
    hash_parts(&[
        trace.project,
        trace.agent,
        trace.command,
        trace.context,
        trace.outcome,
        trace.error_summary,
        trace.fix_summary,
        trace.tags_json,
        if trace.success { "success" } else { "failure" },
    ])
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}
