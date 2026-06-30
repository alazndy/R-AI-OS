use super::*;
use rusqlite::{params, Connection, OptionalExtension, Result};

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

