use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Local, TimeZone};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    #[allow(dead_code)]
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageConfidence {
    Exact,
    Estimated,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
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
        check_ollama(),
        check_npm_tool("claude", "Claude Code"),
        check_npm_tool("gemini", "Gemini CLI"),
        check_cursor(),
        check_lm_studio(),
        check_antigravity(),
    ];

    AiAuditReport {
        tools,
        env_keys: scan_env_keys(),
        local_models: scan_local_models(),
        usage: scan_usage(),
    }
}

fn scan_usage() -> Vec<UsageSnapshot> {
    vec![
        scan_codex_usage(),
        scan_claude_usage(),
        scan_gemini_usage(),
        scan_antigravity_usage(),
    ]
}

fn check_ollama() -> SystemAiTool {
    let output = Command::new("ollama").arg("list").output();
    match output {
        Ok(out) if out.status.success() => SystemAiTool {
            name: "Ollama (Local LLM)".into(),
            status: ToolStatus::Running,
            version: Some("Active".into()),
            path: None,
        },
        _ => SystemAiTool {
            name: "Ollama".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

fn check_npm_tool(cmd: &str, name: &str) -> SystemAiTool {
    match crate::core::process::resolve_command_path(cmd) {
        Some(path) => SystemAiTool {
            name: name.into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(path),
        },
        None => SystemAiTool {
            name: name.into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        },
    }
}

fn check_cursor() -> SystemAiTool {
    if let Some(p) = find_existing_path(&cursor_candidates()) {
        SystemAiTool {
            name: "Cursor IDE".into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "Cursor IDE".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}

fn check_lm_studio() -> SystemAiTool {
    if let Some(p) = find_existing_path(&lm_studio_candidates()) {
        SystemAiTool {
            name: "LM Studio".into(),
            status: ToolStatus::Installed,
            version: None,
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "LM Studio".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}

fn find_existing_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.exists()).cloned()
}

fn cursor_candidates() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut paths = Vec::new();

    if let Some(path) = crate::core::process::resolve_command_path("cursor") {
        paths.push(path);
    }

    paths.push(home.join("AppData/Local/Programs/cursor/Cursor.exe"));
    paths.push(PathBuf::from("/Applications/Cursor.app"));
    paths.push(home.join("Applications/Cursor.app"));
    paths.push(PathBuf::from("/usr/bin/cursor"));
    paths.push(PathBuf::from("/usr/local/bin/cursor"));
    paths.push(home.join(".local/bin/cursor"));
    paths
}

fn lm_studio_candidates() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut paths = Vec::new();

    if let Some(path) = crate::core::process::resolve_command_path("lmstudio") {
        paths.push(path);
    }
    if let Some(path) = crate::core::process::resolve_command_path("lm-studio") {
        paths.push(path);
    }

    paths.push(home.join("AppData/Local/LM-Studio/LM Studio.exe"));
    paths.push(PathBuf::from("/Applications/LM Studio.app"));
    paths.push(home.join("Applications/LM Studio.app"));
    paths.push(PathBuf::from("/usr/bin/lmstudio"));
    paths.push(PathBuf::from("/usr/local/bin/lmstudio"));
    paths.push(home.join(".local/bin/lmstudio"));
    paths
}

fn scan_env_keys() -> Vec<String> {
    let mut keys = Vec::new();
    let common = [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ];
    for key in common {
        if std::env::var(key).is_ok() {
            keys.push(key.to_string());
        }
    }
    keys
}

fn scan_local_models() -> Vec<String> {
    let mut models = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();

    let antigravity_home = home.join(".gemini/antigravity");
    let antigravity_cli_home = home.join(".gemini/antigravity-cli");
    let ag_path = if antigravity_home.exists() {
        Some(antigravity_home)
    } else if antigravity_cli_home.exists() {
        Some(antigravity_cli_home)
    } else {
        None
    };

    if let Some(path) = ag_path {
        models.push(format!("Antigravity Home: {}", path.display()));

        let skills_count = std::fs::read_dir(path.join("skills"))
            .map(|d| d.count())
            .unwrap_or(0);
        if skills_count > 0 {
            models.push(format!("Antigravity Skills: {} deployed", skills_count));
        }

        let brain_count = std::fs::read_dir(path.join("brain"))
            .map(|d| d.count())
            .unwrap_or(0);
        if brain_count > 0 {
            models.push(format!(
                "Antigravity Brain: {} sessions stored",
                brain_count
            ));
        }

        let knowledge_count = std::fs::read_dir(path.join("knowledge"))
            .map(|d| d.count())
            .unwrap_or(0);
        if knowledge_count > 0 {
            models.push(format!(
                "Antigravity Knowledge: {} items cached",
                knowledge_count
            ));
        }
    }

    let ollama_path = home.join(".ollama/models");
    if ollama_path.exists() {
        models.push(format!("Ollama: {}", ollama_path.display()));
    }

    let hf_path = home.join(".cache/huggingface/hub");
    if hf_path.exists() {
        models.push(format!("HuggingFace Cache: {}", hf_path.display()));
    }

    models
}

fn check_antigravity() -> SystemAiTool {
    let home = dirs::home_dir().unwrap_or_default();
    let old_home = home.join(".gemini/antigravity");
    let cli_home = home.join(".gemini/antigravity-cli");
    let path = crate::core::process::resolve_command_path("antigravity")
        .or_else(|| old_home.exists().then_some(old_home.clone()))
        .or_else(|| cli_home.exists().then_some(cli_home.clone()));

    if let Some(p) = path {
        SystemAiTool {
            name: "Antigravity Assistant".into(),
            status: ToolStatus::Running,
            version: Some("Active".into()),
            path: Some(p),
        }
    } else {
        SystemAiTool {
            name: "Antigravity Assistant".into(),
            status: ToolStatus::Missing,
            version: None,
            path: None,
        }
    }
}

fn scan_codex_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let auth_path = home.join(".codex/auth.json");
    let installed = crate::core::process::resolve_command_path("codex").is_some()
        || home.join(".codex").exists();
    let mut usage = UsageSnapshot::new("Codex / OpenAI", installed);

    if std::env::var_os("OPENAI_API_KEY").is_some() {
        usage.authenticated = true;
        usage.plan = Some("api_key".into());
        usage.quota_kind = "api".into();
        usage.source = UsageSource::Env;
        usage.confidence = UsageConfidence::Estimated;
        usage
            .notes
            .push("OPENAI_API_KEY mevcut; exact remaining/reset bu yerel taramada yok.".into());
    }

    if let Some(json) = read_json_value(&auth_path) {
        apply_codex_auth_metadata(&mut usage, &json);
    }

    usage
}

fn scan_claude_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let creds_path = home.join(".claude/.credentials.json");
    let installed = crate::core::process::resolve_command_path("claude").is_some()
        || home.join(".claude").exists();
    let mut usage = UsageSnapshot::new("Claude Code", installed);

    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        usage.authenticated = true;
        usage.plan = Some("api_key".into());
        usage.quota_kind = "api".into();
        usage.source = UsageSource::Env;
        usage.confidence = UsageConfidence::Estimated;
        usage.notes.push(
            "ANTHROPIC_API_KEY mevcut; kalan kullanım ve reset bilgisi local metadata'da yok."
                .into(),
        );
    }

    if let Some(json) = read_json_value(&creds_path) {
        apply_claude_auth_metadata(&mut usage, &json);
    }

    usage
}

fn scan_gemini_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let oauth_path = home.join(".gemini/oauth_creds.json");
    let installed = crate::core::process::resolve_command_path("gemini").is_some()
        || home.join(".gemini").exists();
    let mut usage = UsageSnapshot::new("Gemini CLI", installed);

    if std::env::var_os("GEMINI_API_KEY").is_some() || std::env::var_os("GOOGLE_API_KEY").is_some()
    {
        usage.authenticated = true;
        usage.plan = Some("api_key".into());
        usage.quota_kind = "api".into();
        usage.source = UsageSource::Env;
        usage.confidence = UsageConfidence::Estimated;
        usage.notes.push(
            "Gemini API key mevcut; exact remaining/reset için remote quota API entegrasyonu gerekir."
                .into(),
        );
    }

    if let Some(json) = read_json_value(&oauth_path) {
        apply_gemini_auth_metadata(&mut usage, &json);
    }

    usage
}

fn scan_antigravity_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let oauth_path = home.join(".gemini/antigravity-cli/antigravity-oauth-token");
    let installed = crate::core::process::resolve_command_path("antigravity").is_some()
        || home.join(".gemini/antigravity-cli").exists()
        || home.join(".gemini/antigravity").exists();
    let mut usage = UsageSnapshot::new("Antigravity CLI", installed);

    if let Some(json) = read_json_value(&oauth_path) {
        apply_antigravity_auth_metadata(&mut usage, &json);
    } else if installed {
        usage.source = UsageSource::Inferred;
        usage.notes.push(
            "Antigravity kurulu görünüyor; local auth metadata bulunamadığı için quota durumu bilinmiyor."
                .into(),
        );
    }

    usage
}

fn apply_codex_auth_metadata(usage: &mut UsageSnapshot, auth: &Value) {
    usage.authenticated = true;
    usage.source = UsageSource::LocalAuth;

    if let Some(mode) = auth.get("auth_mode").and_then(Value::as_str) {
        usage.quota_kind = match mode {
            "chatgpt" => "subscription",
            "api" => "api",
            _ => "unknown",
        }
        .into();
    }

    if let Some(last_refresh) = auth.get("last_refresh").and_then(Value::as_str) {
        usage
            .notes
            .push(format!("Local auth yenileme zamanı: {}", last_refresh));
    }

    let claims = auth
        .get("tokens")
        .and_then(|t| t.get("id_token"))
        .and_then(Value::as_str)
        .and_then(decode_jwt_claims)
        .or_else(|| {
            auth.get("tokens")
                .and_then(|t| t.get("access_token"))
                .and_then(Value::as_str)
                .and_then(decode_jwt_claims)
        });

    if let Some(claims) = claims {
        if let Some(openai_auth) = claims.get("https://api.openai.com/auth") {
            if let Some(plan) = openai_auth.get("chatgpt_plan_type").and_then(Value::as_str) {
                usage.plan = Some(plan.into());
            }
            if let Some(active_until) = openai_auth
                .get("chatgpt_subscription_active_until")
                .and_then(Value::as_str)
            {
                usage.renews_at = Some(active_until.into());
            }
        }

        if let Some(exp) = claims.get("exp").and_then(value_to_i64) {
            usage.auth_expires_at = format_epoch_secs(exp);
        }
    }

    if usage.plan.is_some() || usage.renews_at.is_some() || usage.auth_expires_at.is_some() {
        usage.confidence = UsageConfidence::Estimated;
    }

    usage.notes.push(
        "Local Codex auth metadata plan ve auth expiry veriyor; remaining/reset bilgisi vermiyor."
            .into(),
    );
}

fn apply_claude_auth_metadata(usage: &mut UsageSnapshot, auth: &Value) {
    let Some(oauth) = auth.get("claudeAiOauth") else {
        return;
    };

    usage.authenticated = true;
    usage.source = UsageSource::LocalAuth;
    usage.quota_kind = "subscription".into();
    usage.confidence = UsageConfidence::Estimated;

    if let Some(plan) = oauth.get("subscriptionType").and_then(Value::as_str) {
        usage.plan = Some(plan.into());
    }

    if let Some(exp) = oauth.get("expiresAt").and_then(value_to_i64) {
        usage.auth_expires_at = format_epoch_millis(exp);
    }

    if let Some(tier) = oauth.get("rateLimitTier").and_then(Value::as_str) {
        usage.notes.push(format!("Rate limit tier: {}", tier));
    }

    usage.notes.push(
        "Claude local credentials plan ve token expiry gösteriyor; kalan kullanım yerelden okunmuyor."
            .into(),
    );
}

fn apply_gemini_auth_metadata(usage: &mut UsageSnapshot, auth: &Value) {
    usage.authenticated = true;
    usage.source = UsageSource::LocalAuth;
    usage.quota_kind = "oauth".into();
    usage.confidence = UsageConfidence::Estimated;

    if let Some(exp) = auth.get("expiry_date").and_then(value_to_i64) {
        usage.auth_expires_at = format_epoch_millis(exp);
    }

    usage.notes.push(
        "Gemini OAuth metadata bulundu; exact remaining/reset için Google quota telemetry entegrasyonu gerekir."
            .into(),
    );
}

fn apply_antigravity_auth_metadata(usage: &mut UsageSnapshot, auth: &Value) {
    usage.authenticated = true;
    usage.source = UsageSource::LocalAuth;
    usage.quota_kind = "oauth".into();
    usage.confidence = UsageConfidence::Estimated;

    if let Some(method) = auth.get("auth_method").and_then(Value::as_str) {
        usage.plan = Some(method.into());
    }

    if let Some(expiry) = auth
        .get("token")
        .and_then(|token| token.get("expiry"))
        .and_then(Value::as_str)
    {
        usage.auth_expires_at = Some(expiry.into());
    }

    usage.notes.push(
        "Antigravity OAuth token expiry görüldü; kalan kullanım ve reset bilgisi local metadata'da yok."
            .into(),
    );
}

fn read_json_value(path: &Path) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn value_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
}

fn format_epoch_secs(epoch_secs: i64) -> Option<String> {
    Local
        .timestamp_opt(epoch_secs, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
}

fn format_epoch_millis(epoch_millis: i64) -> Option<String> {
    Local
        .timestamp_millis_opt(epoch_millis)
        .single()
        .map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fake_jwt(payload: Value) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(payload.to_string())
        )
    }

    #[test]
    fn decode_jwt_claims_reads_payload() {
        let token = fake_jwt(json!({
            "exp": 1_781_019_153_i64,
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "plus",
                "chatgpt_subscription_active_until": "2026-07-08T19:34:19+00:00"
            }
        }));

        let claims = decode_jwt_claims(&token).expect("claims should decode");
        assert_eq!(
            claims["https://api.openai.com/auth"]["chatgpt_plan_type"],
            "plus"
        );
    }

    #[test]
    fn codex_auth_metadata_extracts_plan_and_dates() {
        let mut usage = UsageSnapshot::new("Codex / OpenAI", true);
        let auth = json!({
            "auth_mode": "chatgpt",
            "last_refresh": "2026-06-09T14:32:33.896340872Z",
            "tokens": {
                "id_token": fake_jwt(json!({
                    "exp": 1_781_019_153_i64,
                    "https://api.openai.com/auth": {
                        "chatgpt_plan_type": "plus",
                        "chatgpt_subscription_active_until": "2026-07-08T19:34:19+00:00"
                    }
                }))
            }
        });

        apply_codex_auth_metadata(&mut usage, &auth);

        assert!(usage.authenticated);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert_eq!(
            usage.renews_at.as_deref(),
            Some("2026-07-08T19:34:19+00:00")
        );
        assert!(usage.auth_expires_at.is_some());
    }

    #[test]
    fn claude_auth_metadata_extracts_subscription_type() {
        let mut usage = UsageSnapshot::new("Claude Code", true);
        let auth = json!({
            "claudeAiOauth": {
                "subscriptionType": "pro",
                "rateLimitTier": "default_claude_ai",
                "expiresAt": 1_781_044_314_708_i64
            }
        });

        apply_claude_auth_metadata(&mut usage, &auth);

        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert!(usage.auth_expires_at.is_some());
        assert!(usage
            .notes
            .iter()
            .any(|note| note.contains("Rate limit tier")));
    }

    #[test]
    fn gemini_auth_metadata_extracts_oauth_expiry() {
        let mut usage = UsageSnapshot::new("Gemini CLI", true);
        let auth = json!({
            "expiry_date": 1_781_086_385_068_i64
        });

        apply_gemini_auth_metadata(&mut usage, &auth);

        assert!(usage.authenticated);
        assert!(usage.auth_expires_at.is_some());
        assert_eq!(usage.quota_kind, "oauth");
    }

    #[test]
    fn antigravity_auth_metadata_extracts_expiry() {
        let mut usage = UsageSnapshot::new("Antigravity CLI", true);
        let auth = json!({
            "auth_method": "consumer",
            "token": {
                "expiry": "2026-06-09T18:34:34.833463712+03:00"
            }
        });

        apply_antigravity_auth_metadata(&mut usage, &auth);

        assert_eq!(usage.plan.as_deref(), Some("consumer"));
        assert_eq!(
            usage.auth_expires_at.as_deref(),
            Some("2026-06-09T18:34:34.833463712+03:00")
        );
    }
}
