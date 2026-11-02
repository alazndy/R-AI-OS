use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

// ─── Lease record ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct SecretLease {
    pub id: String,
    pub tool: String,
    pub env_var: String,
    pub granted_at: String,
    pub expires_at: String,
    pub status: String,
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SecretLeaseError {
    NotFound,
    Expired,
    Db(rusqlite::Error),
}

impl std::fmt::Display for SecretLeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(
                f,
                "secret_lease: no active lease found for this tool/env_var pair"
            ),
            Self::Expired => write!(
                f,
                "secret_lease: lease has expired — run `raios secret grant` again"
            ),
            Self::Db(e) => write!(f, "secret_lease: db error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for SecretLeaseError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

// ─── TTL parsing ─────────────────────────────────────────────────────────────

/// Parse a human duration string into seconds.
/// Supported suffixes: s, m, h, d.  Plain number = seconds.
pub fn parse_ttl(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty duration".to_string());
    }
    let (digits, suffix) = s.split_at(s.len() - 1);
    let multiplier: u64 = match suffix {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        c if c.chars().all(|ch| ch.is_ascii_digit()) => {
            return s
                .parse::<u64>()
                .map_err(|_| format!("invalid duration: {s}"));
        }
        _ => return Err(format!("unknown duration suffix '{suffix}' — use s/m/h/d")),
    };
    digits
        .parse::<u64>()
        .map(|n| n * multiplier)
        .map_err(|_| format!("invalid duration: {s}"))
}

// ─── DB helpers ──────────────────────────────────────────────────────────────

pub fn ensure_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS secret_leases (
            id          TEXT PRIMARY KEY,
            tool        TEXT NOT NULL,
            env_var     TEXT NOT NULL,
            granted_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL,
            status      TEXT NOT NULL DEFAULT 'active'
        );
        CREATE INDEX IF NOT EXISTS idx_secret_leases_tool ON secret_leases(tool, status);",
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn new_id() -> String {
    let now = now_secs();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{now:x}{nanos:08x}")
}

fn unix_to_iso(secs: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let t = UNIX_EPOCH + Duration::from_secs(secs);
    let secs_in_day = secs % 86400;
    let days = secs / 86400;
    let _ = t;
    // Simple ISO-ish display without chrono
    let h = secs_in_day / 3600;
    let m = (secs_in_day % 3600) / 60;
    let s = secs_in_day % 60;
    // Days since epoch (approx)
    let days_since_epoch = days;
    // 1970-01-01 + days
    let year = 1970 + days_since_epoch / 365;
    let doy = days_since_epoch % 365;
    let month = doy / 30 + 1;
    let day = doy % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Grant a new secret lease. Returns the lease ID.
pub fn grant(
    conn: &Connection,
    tool: &str,
    env_var: &str,
    ttl_secs: u64,
) -> rusqlite::Result<String> {
    ensure_table(conn)?;
    let id = new_id();
    let now = now_secs();
    let expires = now + ttl_secs;
    conn.execute(
        "INSERT INTO secret_leases (id, tool, env_var, granted_at, expires_at) VALUES (?1,?2,?3,?4,?5)",
        params![id, tool, env_var, now as i64, expires as i64],
    )?;
    Ok(id)
}

/// Revoke an active lease. Returns true if a row was updated.
pub fn revoke(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE secret_leases SET status='revoked' WHERE id=?1 AND status='active'",
        params![id],
    )?;
    Ok(n > 0)
}

/// Mark all leases whose expires_at < now as 'expired'. Returns count updated.
pub fn expire_old(conn: &Connection) -> rusqlite::Result<usize> {
    ensure_table(conn)?;
    let now = now_secs() as i64;
    let n = conn.execute(
        "UPDATE secret_leases SET status='expired' WHERE status='active' AND expires_at < ?1",
        params![now],
    )?;
    Ok(n)
}

/// List all active (non-expired) leases.
pub fn list_active(conn: &Connection) -> rusqlite::Result<Vec<SecretLease>> {
    ensure_table(conn)?;
    expire_old(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id,tool,env_var,granted_at,expires_at,status FROM secret_leases
         WHERE status='active' ORDER BY expires_at ASC",
    )?;
    collect_rows(&mut stmt)
}

/// List all leases (last 50).
pub fn list_all(conn: &Connection) -> rusqlite::Result<Vec<SecretLease>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id,tool,env_var,granted_at,expires_at,status FROM secret_leases
         ORDER BY granted_at DESC LIMIT 50",
    )?;
    collect_rows(&mut stmt)
}

fn collect_rows(stmt: &mut rusqlite::Statement<'_>) -> rusqlite::Result<Vec<SecretLease>> {
    let rows = stmt.query_map([], |r| {
        Ok(SecretLease {
            id: r.get(0)?,
            tool: r.get(1)?,
            env_var: r.get(2)?,
            granted_at: {
                let v: i64 = r.get(3)?;
                unix_to_iso(v as u64)
            },
            expires_at: {
                let v: i64 = r.get(4)?;
                unix_to_iso(v as u64)
            },
            status: r.get(5)?,
        })
    })?;
    Ok(rows.flatten().collect())
}

// ─── Vault: leasable secrets are not ambient ─────────────────────────────────

/// Values of every env var that appears in a lease row, moved out of the
/// process environment by `scrub_leasable_env`. Outside a leased tool call
/// the secret is therefore invisible to this process and to any child it
/// spawns (build/test scripts, git hooks, …).
fn vault() -> &'static Mutex<BTreeMap<String, String>> {
    static VAULT: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();
    VAULT.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn vault_lock() -> std::sync::MutexGuard<'static, BTreeMap<String, String>> {
    vault().lock().unwrap_or_else(|e| e.into_inner())
}

/// Moves every env var named in any lease row (active, expired or revoked —
/// revoking must not make a secret ambient again) from the process
/// environment into the vault. Returns how many variables were moved.
///
/// Call this before each tool dispatch: it also covers grants made after the
/// server started. Not thread-safe against concurrent `getenv` in other
/// threads; the MCP stdio server dispatches sequentially.
pub fn scrub_leasable_env(conn: &Connection) -> usize {
    let vars: Vec<String> = match conn.prepare("SELECT DISTINCT env_var FROM secret_leases") {
        Ok(mut stmt) => match stmt.query_map([], |r| r.get(0)) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => return 0,
        },
        Err(_) => return 0,
    };
    let mut moved = 0;
    let mut vault = vault_lock();
    for var in vars {
        if let Some(val) = std::env::var_os(&var) {
            if let Some(val) = val.to_str() {
                vault.insert(var.clone(), val.to_string());
            }
            std::env::remove_var(&var);
            moved += 1;
        }
    }
    moved
}

/// Restores the ambient environment when a leased tool call finishes.
#[must_use = "dropping the guard immediately removes the leased variables"]
pub struct LeaseEnvGuard {
    vars: Vec<String>,
}

impl Drop for LeaseEnvGuard {
    fn drop(&mut self) {
        for var in &self.vars {
            std::env::remove_var(var);
        }
    }
}

/// Scrubs leasable secrets, then exposes only the ones actively leased to
/// `tool` for as long as the returned guard lives.
pub fn inject_active_env(conn: &Connection, tool: &str) -> LeaseEnvGuard {
    scrub_leasable_env(conn);
    let pairs = active_env_for_tool(conn, tool);
    let mut vars = Vec::with_capacity(pairs.len());
    for (var, val) in pairs {
        std::env::set_var(&var, val);
        vars.push(var);
    }
    LeaseEnvGuard { vars }
}

/// Collect active env var values for a tool: from the vault when the secret
/// has been scrubbed, otherwise from the host process's environment.
/// Skips env vars with no value anywhere (grants without a value are ignored).
pub fn active_env_for_tool(conn: &Connection, tool: &str) -> Vec<(String, String)> {
    expire_old(conn).ok();
    let now = now_secs() as i64;
    let mut stmt = match conn.prepare(
        "SELECT env_var FROM secret_leases WHERE tool=?1 AND status='active' AND expires_at >= ?2",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let vars: Vec<String> = match stmt.query_map(params![tool, now], |r| r.get(0)) {
        Ok(rows) => rows.flatten().collect(),
        Err(_) => vec![],
    };

    let vault = vault_lock();
    vars.into_iter()
        .filter_map(|var| {
            let val = vault
                .get(&var)
                .cloned()
                .or_else(|| std::env::var(&var).ok())?;
            Some((var, val))
        })
        .collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_table(&conn).unwrap();
        conn
    }

    #[test]
    fn parse_ttl_seconds() {
        assert_eq!(parse_ttl("30s").unwrap(), 30);
        assert_eq!(parse_ttl("5m").unwrap(), 300);
        assert_eq!(parse_ttl("2h").unwrap(), 7200);
        assert_eq!(parse_ttl("1d").unwrap(), 86400);
        assert_eq!(parse_ttl("120").unwrap(), 120);
    }

    #[test]
    fn parse_ttl_invalid() {
        assert!(parse_ttl("abc").is_err());
        assert!(parse_ttl("5x").is_err());
        assert!(parse_ttl("").is_err());
    }

    #[test]
    fn grant_and_list_active() {
        let conn = mem_conn();
        grant(&conn, "git_commit", "GITHUB_TOKEN", 300).unwrap();
        let leases = list_active(&conn).unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].tool, "git_commit");
        assert_eq!(leases[0].env_var, "GITHUB_TOKEN");
        assert_eq!(leases[0].status, "active");
    }

    #[test]
    fn revoke_removes_from_active() {
        let conn = mem_conn();
        let id = grant(&conn, "run_tests", "CI_TOKEN", 3600).unwrap();
        assert_eq!(list_active(&conn).unwrap().len(), 1);
        assert!(revoke(&conn, &id).unwrap());
        assert_eq!(list_active(&conn).unwrap().len(), 0);
    }

    #[test]
    fn expired_lease_not_listed() {
        let conn = mem_conn();
        // Grant with 0-second TTL (already expired)
        let now = now_secs() as i64;
        let id = new_id();
        conn.execute(
            "INSERT INTO secret_leases (id,tool,env_var,granted_at,expires_at) VALUES (?1,?2,?3,?4,?5)",
            params![id, "run_build", "BUILD_KEY", now, now - 1],
        ).unwrap();
        assert_eq!(list_active(&conn).unwrap().len(), 0);
    }

    #[test]
    fn multiple_leases_for_same_tool() {
        let conn = mem_conn();
        grant(&conn, "git_commit", "GITHUB_TOKEN", 300).unwrap();
        grant(&conn, "git_commit", "GPG_KEY", 600).unwrap();
        let leases = list_active(&conn).unwrap();
        assert_eq!(leases.len(), 2);
    }

    #[test]
    fn list_all_includes_revoked() {
        let conn = mem_conn();
        let id = grant(&conn, "run_tests", "API_KEY", 300).unwrap();
        revoke(&conn, &id).unwrap();
        let all = list_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "revoked");
    }

    #[test]
    fn active_env_for_tool_returns_present_vars() {
        let conn = mem_conn();
        // Set a known env var in the test process
        std::env::set_var("RAIOS_TEST_SECRET", "test_value_42");
        grant(&conn, "test_tool", "RAIOS_TEST_SECRET", 300).unwrap();
        grant(&conn, "test_tool", "RAIOS_DEFINITELY_NOT_SET_XYZ", 300).unwrap();
        let pairs = active_env_for_tool(&conn, "test_tool");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "RAIOS_TEST_SECRET");
        assert_eq!(pairs[0].1, "test_value_42");
        std::env::remove_var("RAIOS_TEST_SECRET");
    }

    #[test]
    fn leased_secret_is_scrubbed_and_only_visible_to_its_tool_during_the_call() {
        let conn = mem_conn();
        let var = "RAIOS_TEST_SCRUB_SECRET";
        std::env::set_var(var, "s3cret");
        grant(&conn, "tool_a", var, 300).unwrap();

        // Another tool's call: the secret must not be ambient.
        {
            let _g = inject_active_env(&conn, "tool_b");
            assert!(
                std::env::var(var).is_err(),
                "secret leaked to unleased tool"
            );
        }
        // The leased tool sees it for exactly the guard's lifetime.
        {
            let _g = inject_active_env(&conn, "tool_a");
            assert_eq!(std::env::var(var).as_deref(), Ok("s3cret"));
        }
        assert!(
            std::env::var(var).is_err(),
            "secret stayed ambient after call"
        );
    }

    #[test]
    fn revoked_lease_does_not_make_secret_ambient_again() {
        let conn = mem_conn();
        let var = "RAIOS_TEST_REVOKED_SECRET";
        std::env::set_var(var, "v");
        let id = grant(&conn, "tool_a", var, 300).unwrap();
        revoke(&conn, &id).unwrap();

        let _g = inject_active_env(&conn, "tool_a");
        assert!(std::env::var(var).is_err());
    }
}
