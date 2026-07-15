use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorTier {
    Offline,
    Auth,
    Full,
}

impl DoctorTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            DoctorTier::Offline => "offline",
            DoctorTier::Auth => "auth",
            DoctorTier::Full => "full",
        }
    }
}

impl std::str::FromStr for DoctorTier {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "full" => DoctorTier::Full,
            "auth" => DoctorTier::Auth,
            _ => DoctorTier::Offline,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DoctorResult {
    pub agent: String,
    pub tier_reached: DoctorTier,
    pub notes: Vec<String>,
    pub checked_at: String,
}

pub fn save_doctor_result(conn: &Connection, res: &DoctorResult) -> Result<()> {
    let notes_json = serde_json::to_string(&res.notes).unwrap_or_else(|_| "[]".into());
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    conn.execute(
        "INSERT INTO agent_doctor_runs (agent, tier_reached, notes_json, checked_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(agent) DO UPDATE SET
            tier_reached = excluded.tier_reached,
            notes_json   = excluded.notes_json,
            checked_at   = excluded.checked_at",
        params![res.agent, res.tier_reached.as_str(), notes_json, now],
    )?;
    Ok(())
}

pub fn get_doctor_result(conn: &Connection, agent: &str) -> Result<Option<DoctorResult>> {
    conn.query_row(
        "SELECT agent, tier_reached, notes_json, checked_at FROM agent_doctor_runs WHERE agent = ?1",
        params![agent],
        |row| {
            let tier_str: String = row.get(1)?;
            let notes_json: String = row.get(2)?;
            let notes: Vec<String> = serde_json::from_str(&notes_json).unwrap_or_default();
            let tier_reached = tier_str.parse().unwrap_or(DoctorTier::Offline);
            Ok(DoctorResult {
                agent: row.get(0)?,
                tier_reached,
                notes,
                checked_at: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn run_doctor_check(agent: &str, requested_tier: Option<DoctorTier>) -> DoctorResult {
    let canonical_agent = match agent.to_lowercase().as_str() {
        "claude" | "claude_kaira" => "claude",
        "codex" | "codex_kaira" => "codex",
        "opencode" | "opencode_kaira" => "opencode",
        "agy" | "antigravity" | "antigravity_kaira" => "agy",
        _ => agent,
    };

    let target_tier = requested_tier.unwrap_or(DoctorTier::Full);
    let home = dirs::home_dir().unwrap_or_default();
    let mut notes = Vec::new();
    let mut tier_reached = DoctorTier::Offline;

    // 1. Offline Tier: Check installation / binary path
    let binary_name = canonical_agent;

    let binary_found = raios_core::core::process::resolve_command_path(binary_name).is_some();
    let config_dir_found = match canonical_agent {
        "claude" => home.join(".claude").exists(),
        "codex" => home.join(".codex").exists(),
        "opencode" => home.join(".config/opencode").exists(),
        "agy" => home.join(".gemini/antigravity-cli").exists(),
        _ => false,
    };

    if !binary_found && !config_dir_found {
        notes.push(format!("Binary '{}' not found on PATH and config dir missing", binary_name));
        return DoctorResult {
            agent: canonical_agent.to_string(),
            tier_reached: DoctorTier::Offline,
            notes,
            checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
    }

    notes.push(format!("Offline check passed: binary '{}' detected", binary_name));

    if target_tier == DoctorTier::Offline {
        return DoctorResult {
            agent: canonical_agent.to_string(),
            tier_reached,
            notes,
            checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
    }

    // 2. Auth Tier: Check credentials / API key / cache age
    let auth_valid = match canonical_agent {
        "claude" => {
            let env_key = std::env::var_os("ANTHROPIC_API_KEY").is_some();
            let creds_path = home.join(".claude/.credentials.json");
            let cache_path = home.join(".claude/raios-usage-cache.json");

            if env_key {
                notes.push("ANTHROPIC_API_KEY environment variable set".into());
            }

            if creds_path.exists() {
                if check_file_stale(&creds_path, 24) {
                    notes.push("Credentials file found but older than 24h: stale, re-check needed".into());
                } else {
                    notes.push("Fresh credentials file found (.credentials.json)".into());
                }
            }

            if cache_path.exists() && check_file_stale(&cache_path, 24) {
                notes.push("Usage cache file is older than 24h: stale, re-check".into());
            }

            env_key || creds_path.exists()
        }
        "codex" => {
            let env_key = std::env::var_os("OPENAI_API_KEY").is_some();
            let auth_path = home.join(".codex/auth.json");

            if env_key {
                notes.push("OPENAI_API_KEY environment variable set".into());
            }

            if auth_path.exists() {
                if check_file_stale(&auth_path, 24) {
                    notes.push("Codex auth file found but older than 24h: stale, re-check needed".into());
                } else {
                    notes.push("Fresh auth file found (auth.json)".into());
                }
            }

            env_key || auth_path.exists()
        }
        "agy" => {
            let token_path = home.join(".gemini/antigravity-cli/antigravity-oauth-token");
            if token_path.exists() {
                if check_file_stale(&token_path, 24) {
                    notes.push("Antigravity token file older than 24h: stale, re-check needed".into());
                } else {
                    notes.push("Fresh Antigravity OAuth token file found".into());
                }
                true
            } else {
                notes.push("No OAuth token found in ~/.gemini/antigravity-cli/".into());
                false
            }
        }
        _ => true,
    };

    if !auth_valid {
        notes.push("Auth check failed: credentials missing or expired".into());
        return DoctorResult {
            agent: canonical_agent.to_string(),
            tier_reached,
            notes,
            checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
    }

    tier_reached = DoctorTier::Auth;
    if target_tier == DoctorTier::Auth {
        return DoctorResult {
            agent: canonical_agent.to_string(),
            tier_reached,
            notes,
            checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
    }

    // 3. Full Tier: Verify CLI binary execution
    let output = Command::new(binary_name).arg("--version").output();
    match output {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
            notes.push(format!("Full roundtrip check succeeded: {}", ver));
            tier_reached = DoctorTier::Full;
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            notes.push(format!("Full roundtrip --version returned non-zero: {}", err));
        }
        Err(e) => {
            notes.push(format!("Full roundtrip execution failed to spawn: {}", e));
        }
    }

    DoctorResult {
        agent: canonical_agent.to_string(),
        tier_reached,
        notes,
        checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

fn check_file_stale(path: &PathBuf, max_hours: i64) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(mtime) = metadata.modified() else {
        return true;
    };
    let Ok(elapsed) = mtime.elapsed() else {
        return false;
    };
    elapsed.as_secs() > (max_hours as u64 * 3600)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_check_missing_binary_returns_offline_tier() {
        let res = run_doctor_check("non_existent_fake_agent_12345", None);
        assert_eq!(res.tier_reached, DoctorTier::Offline);
        assert!(res.notes.iter().any(|n| n.contains("not found")));
    }

    #[test]
    fn doctor_result_db_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let res = DoctorResult {
            agent: "claude".into(),
            tier_reached: DoctorTier::Auth,
            notes: vec!["Offline check passed".into(), "Stale auth check flagged".into()],
            checked_at: "2026-07-15 12:00:00".into(),
        };

        save_doctor_result(&conn, &res).unwrap();

        let loaded = get_doctor_result(&conn, "claude").unwrap().unwrap();
        assert_eq!(loaded.agent, "claude");
        assert_eq!(loaded.tier_reached, DoctorTier::Auth);
        assert_eq!(loaded.notes.len(), 2);
    }
}
