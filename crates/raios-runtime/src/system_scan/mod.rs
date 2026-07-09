use serde::Serialize;
use std::path::PathBuf;

mod tools;
mod usage;

#[derive(Debug, Clone, Serialize)]
pub struct SystemAiTool {
    pub name: String,
    pub status: ToolStatus,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ToolStatus {
    Running,
    Installed,
    Missing,
    /// Matched in the TUI (ui/health.rs) but never constructed today — no
    /// scan path currently distinguishes "detection failed" from "not
    /// installed". Reserved for when one does, rather than removed and
    /// re-added later.
    #[allow(dead_code)]
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageConfidence {
    Exact,
    Estimated,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    LocalAuth,
    Env,
    LocalLog,
    Inferred,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageSnapshot {
    pub provider: String,
    pub installed: bool,
    pub authenticated: bool,
    pub plan: Option<String>,
    pub quota_kind: String,
    pub used: Option<String>,
    pub remaining: Option<String>,
    pub reset_at: Option<String>,
    pub renews_at: Option<String>,
    pub auth_expires_at: Option<String>,
    pub confidence: UsageConfidence,
    pub source: UsageSource,
    pub notes: Vec<String>,
}

impl UsageSnapshot {
    fn new(provider: &str, installed: bool) -> Self {
        Self {
            provider: provider.into(),
            installed,
            authenticated: false,
            plan: None,
            quota_kind: "unknown".into(),
            used: None,
            remaining: None,
            reset_at: None,
            renews_at: None,
            auth_expires_at: None,
            confidence: UsageConfidence::Unavailable,
            source: UsageSource::Unavailable,
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AiAuditReport {
    pub tools: Vec<SystemAiTool>,
    pub env_keys: Vec<String>,
    pub local_models: Vec<String>,
    pub usage: Vec<UsageSnapshot>,
}

pub fn scan_system() -> AiAuditReport {
    let tools = vec![
        tools::check_ollama(),
        tools::check_npm_tool("claude", "Claude Code"),
        tools::check_cursor(),
        tools::check_lm_studio(),
        tools::check_antigravity(),
        tools::check_opencode(),
    ];

    AiAuditReport {
        tools,
        env_keys: tools::scan_env_keys(),
        local_models: tools::scan_local_models(),
        usage: scan_usage(),
    }
}

fn scan_usage() -> Vec<UsageSnapshot> {
    vec![
        usage::scan_codex_usage(),
        usage::scan_claude_usage(),
        usage::scan_antigravity_usage(),
        usage::scan_opencode_usage(),
    ]
}
