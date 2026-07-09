use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Local, TimeZone};
use serde_json::Value;
use std::fs;
use std::path::Path;

use super::{UsageConfidence, UsageSnapshot, UsageSource};

const USAGE_CACHE_STALENESS_HOURS: i64 = 24;

pub(super) fn scan_codex_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let auth_path = home.join(".codex/auth.json");
    let installed = raios_core::core::process::resolve_command_path("codex").is_some()
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

pub(super) fn scan_claude_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let creds_path = home.join(".claude/.credentials.json");
    let installed = raios_core::core::process::resolve_command_path("claude").is_some()
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

    let cache_path = home.join(".claude/raios-usage-cache.json");
    apply_usage_cache(&mut usage, &cache_path);

    usage
}

pub(super) fn scan_opencode_usage() -> UsageSnapshot {
    let installed = raios_core::core::process::resolve_command_path("opencode").is_some();
    let mut usage = UsageSnapshot::new("OpenCode", installed);

    if installed {
        usage.source = UsageSource::Inferred;
        usage.notes.push(
            "OpenCode kurulu; local config üzerinden kullanım takibi yapılıyor.".into(),
        );
    }

    usage
}

pub(super) fn scan_antigravity_usage() -> UsageSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let auth_path = home.join(".gemini/antigravity-cli/antigravity-oauth-token");
    let installed = raios_core::core::process::resolve_command_path("antigravity").is_some()
        || auth_path.exists();
    let mut usage = UsageSnapshot::new("Antigravity CLI", installed);

    if let Some(json) = read_json_value(&auth_path) {
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

fn apply_usage_cache(usage: &mut UsageSnapshot, cache_path: &Path) {
    let Ok(content) = fs::read_to_string(cache_path) else {
        usage.notes.push(
            "Kalan kullanım cache dosyası bulunamadı — no active/recent session to source from."
                .into(),
        );
        return;
    };
    let Ok(cache) = serde_json::from_str::<Value>(&content) else {
        usage.notes.push(
            "Kalan kullanım cache dosyası okunamadı (bozuk JSON) — no active/recent session to source from."
                .into(),
        );
        return;
    };

    let updated_at = cache.get("updated_at").and_then(Value::as_str);
    let is_fresh = updated_at
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| {
            let age = chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
            age.num_hours() < USAGE_CACHE_STALENESS_HOURS
        })
        .unwrap_or(false);

    if !is_fresh {
        usage.notes.push(
            "Kalan kullanım cache'i eski (>24 saat) — no active/recent session to source from."
                .into(),
        );
        return;
    }

    let five = cache.get("five_hour_used_pct").and_then(Value::as_f64);
    let week = cache.get("seven_day_used_pct").and_then(Value::as_f64);

    if five.is_none() && week.is_none() {
        usage.notes.push(
            "Kalan kullanım cache'inde veri yok — no active/recent session to source from.".into(),
        );
        return;
    }

    let mut parts = Vec::new();
    if let Some(v) = five {
        parts.push(format!("5h:{:.0}%", 100.0 - v));
    }
    if let Some(v) = week {
        parts.push(format!("7d:{:.0}%", 100.0 - v));
    }
    usage.remaining = Some(format!("{} remaining", parts.join(" ")));

    let five_reset = cache.get("five_hour_resets_at").and_then(value_to_i64);
    let week_reset = cache.get("seven_day_resets_at").and_then(value_to_i64);
    let mut reset_parts = Vec::new();
    if let Some(reset) = five_reset.and_then(format_epoch_secs) {
        reset_parts.push(format!("5h:{reset}"));
    }
    if let Some(reset) = week_reset.and_then(format_epoch_secs) {
        reset_parts.push(format!("7d:{reset}"));
    }
    if !reset_parts.is_empty() {
        usage.reset_at = Some(reset_parts.join(" "));
    }

    usage.source = UsageSource::LocalLog;
    usage.confidence = UsageConfidence::Estimated;
    usage
        .notes
        .retain(|n| !n.contains("kalan kullanım yerelden okunmuyor"));
    usage.notes.push(format!(
        "cached from statusLine as of {}, not live-polled — only reflects the last active Claude Code session tick.",
        updated_at.unwrap_or("unknown time")
    ));
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
    use std::io::Write;

    fn write_temp_cache(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("raios-usage-cache.json");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

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

    #[test]
    fn apply_usage_cache_populates_remaining_from_fresh_cache() {
        let dir = std::env::temp_dir().join(format!("raios-usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let cache_path = write_temp_cache(
            &dir,
            &format!(
                r#"{{"five_hour_used_pct":18.4,"seven_day_used_pct":39.0,"five_hour_resets_at":1999999999,"seven_day_resets_at":2000500000,"updated_at":"{}"}}"#,
                now
            ),
        );

        let mut usage = UsageSnapshot::new("Claude Code", true);
        usage.source = UsageSource::LocalAuth;
        usage.plan = Some("pro".into());

        apply_usage_cache(&mut usage, &cache_path);

        assert_eq!(usage.remaining.as_deref(), Some("5h:82% 7d:61% remaining"));
        assert_eq!(usage.source, UsageSource::LocalLog);
        assert_eq!(usage.confidence, UsageConfidence::Estimated);
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert!(usage.notes.iter().any(|n| n.contains("cached from statusLine")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_usage_cache_ignores_stale_cache() {
        let dir = std::env::temp_dir().join(format!(
            "raios-usage-test-stale-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old = (chrono::Utc::now() - chrono::Duration::hours(30)).to_rfc3339();
        let cache_path = write_temp_cache(
            &dir,
            &format!(
                r#"{{"five_hour_used_pct":18.4,"seven_day_used_pct":39.0,"five_hour_resets_at":null,"seven_day_resets_at":null,"updated_at":"{}"}}"#,
                old
            ),
        );

        let mut usage = UsageSnapshot::new("Claude Code", true);
        apply_usage_cache(&mut usage, &cache_path);

        assert_eq!(usage.remaining, None);
        assert!(usage
            .notes
            .iter()
            .any(|n| n.contains("no active/recent session")));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apply_usage_cache_missing_file_is_a_noop_note() {
        let mut usage = UsageSnapshot::new("Claude Code", true);
        apply_usage_cache(
            &mut usage,
            std::path::Path::new("/tmp/does-not-exist-raios-usage-cache.json"),
        );

        assert_eq!(usage.remaining, None);
        assert!(usage
            .notes
            .iter()
            .any(|n| n.contains("no active/recent session")));
    }
}
