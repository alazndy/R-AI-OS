use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;
use std::process::{Command, Stdio};

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
    /// Local credential discovery and CLI execution cannot verify backend auth.
    #[serde(default)]
    pub authentication_verified: bool,
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
            let _legacy_tier = tier_str;
            let tier_reached = DoctorTier::Offline;
            Ok(DoctorResult {
                agent: row.get(0)?,
                tier_reached,
                notes,
                checked_at: row.get(3)?,
                authentication_verified: false,
            })
        },
    )
    .optional()
}

/// Reports whether `tmux` (required for `raios steer` and daemon-spawned
/// agent sessions) is present on `PATH`. Split out from `run_doctor_check` so
/// it can be tested independent of any particular agent's own binary/config
/// presence, which gates the rest of that function.
fn tmux_presence_note() -> String {
    match which::which("tmux") {
        Ok(path) => format!("tmux: found at {}", path.display()),
        Err(_) => "tmux: NOT FOUND on PATH — required for `raios steer` and daemon-spawned \
             agent sessions; install it (e.g. `apt install tmux` / `brew install tmux`)"
            .to_string(),
    }
}

/// A local diagnostic never claims authenticated reachability. No provider
/// model call is made. Auth/full requests expose what remains unverified.
pub fn run_doctor_check(agent: &str, requested_tier: Option<DoctorTier>) -> DoctorResult {
    let canonical_agent = match agent.to_lowercase().as_str() {
        "claude" | "claude_kaira" => "claude",
        "codex" | "codex_kaira" => "codex",
        "opencode" | "opencode_kaira" => "opencode",
        "agy" | "antigravity" | "antigravity_kaira" => "agy",
        _ => agent,
    };
    let target = requested_tier.unwrap_or(DoctorTier::Full);
    let mut result = DoctorResult {
        agent: canonical_agent.into(),
        tier_reached: DoctorTier::Offline,
        notes: vec![],
        checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        authentication_verified: false,
    };
    let Some(binary) = raios_core::core::process::resolve_command_path(canonical_agent) else {
        result
            .notes
            .push(format!("Binary '{canonical_agent}' not found on PATH"));
        return result;
    };
    result.notes.push(format!(
        "Offline check: binary detected at {}",
        binary.display()
    ));
    result.notes.push(tmux_presence_note());
    if target == DoctorTier::Offline {
        return result;
    }
    let home = dirs::home_dir().unwrap_or_default();
    match canonical_agent {
        "claude" => result.notes.push(local_credentials_note(
            std::env::var("ANTHROPIC_API_KEY").is_ok_and(|v| !v.trim().is_empty()),
            home.join(".claude/.credentials.json").is_file(),
        )),
        "codex" => result.notes.push(local_credentials_note(
            std::env::var("OPENAI_API_KEY").is_ok_and(|v| !v.trim().is_empty()),
            home.join(".codex/auth.json").is_file(),
        )),
        // These providers may use their own stores/keyrings. Missing legacy
        // Gemini files, or an unsupported probe, cannot establish auth failure.
        _ => result
            .notes
            .push("Local credential discovery: unverified for this provider".into()),
    }
    result.notes.push("Authentication: unverified; local files and their modification age do not establish token validity".into());
    if target == DoctorTier::Full {
        let arg = if canonical_agent == "agy" {
            "--help"
        } else {
            "--version"
        };
        result.notes.push(match bounded_local_probe(&binary, arg) {
            Ok(true) => "Local CLI execution: successful; authenticated backend reachability remains unverified".into(),
            Ok(false) => "Local CLI execution: nonzero exit; authenticated backend reachability remains unverified".into(),
            Err(e) => format!("Local CLI execution: {e}; authenticated backend reachability remains unverified"),
        });
    }
    result
}

fn local_credentials_note(env_present: bool, file_present: bool) -> String {
    format!(
        "Local credentials: {}; authentication remains unverified",
        if env_present || file_present {
            "detected"
        } else {
            "not detected in the checked locations"
        }
    )
}

fn bounded_local_probe(binary: &Path, arg: &str) -> std::io::Result<bool> {
    let mut child = Command::new(binary)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.success()),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return match result {
                    Err(e) => Err(e),
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "local CLI probe timed out",
                    )),
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_discovery_never_claims_expiration_or_authentication() {
        for (env, file) in [(true, false), (false, true), (false, false)] {
            let note = local_credentials_note(env, file);
            assert!(note.contains("unverified"));
            assert!(!note.contains("expired") && !note.contains("stale"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_successful_local_execution_is_only_a_local_probe() {
        assert!(bounded_local_probe(Path::new("/bin/true"), "--version").unwrap());
        assert!(!bounded_local_probe(Path::new("/bin/false"), "--version").unwrap());
    }

    #[test]
    fn doctor_check_missing_binary_returns_offline_tier() {
        let res = run_doctor_check("non_existent_fake_agent_12345", None);
        assert_eq!(res.tier_reached, DoctorTier::Offline);
        assert!(res.notes.iter().any(|n| n.contains("not found")));
    }

    #[test]
    fn doctor_check_notes_tmux_presence() {
        // Deliberately does not go through `run_doctor_check("claude", ..)`:
        // that gates every check (including this one) behind the "claude"
        // agent's own binary/config-dir presence, which is true on a
        // developer machine running Claude Code but false on a clean CI
        // runner — making this test fail there for a reason that has
        // nothing to do with tmux detection itself.
        let note = tmux_presence_note();
        assert!(
            note.starts_with("tmux:"),
            "expected a tmux presence note, got: {:?}",
            note
        );
    }

    #[test]
    fn doctor_result_db_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();

        let res = DoctorResult {
            agent: "claude".into(),
            tier_reached: DoctorTier::Offline,
            notes: vec![
                "Offline check passed".into(),
                "Stale auth check flagged".into(),
            ],
            checked_at: "2026-07-15 12:00:00".into(),
            authentication_verified: false,
        };

        save_doctor_result(&conn, &res).unwrap();

        let loaded = get_doctor_result(&conn, "claude").unwrap().unwrap();
        assert_eq!(loaded.agent, "claude");
        assert_eq!(loaded.tier_reached, DoctorTier::Offline);
        assert!(!loaded.authentication_verified);
        assert_eq!(loaded.notes.len(), 2);
    }
}
