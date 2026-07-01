use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::json;
use sha2::{Digest, Sha256};

// ─── Public API ───────────────────────────────────────────────────────────────

/// Write a new event to the audit ledger and compute its hash chain entry.
///
/// `hash[n] = SHA256( prev_hash || timestamp || event_type || actor || data )`
pub fn record_audit_event(
    conn: &Connection,
    event_type: &str,
    actor: &str,
    data: &str,
) -> Result<()> {
    // Fetch the most recent hash to build the chain
    let prev_hash: String = conn
        .query_row(
            "SELECT hash FROM audit_log ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or_default(); // Empty string for the genesis entry

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let hash = compute_hash(&prev_hash, &timestamp, event_type, actor, data);

    conn.execute(
        "INSERT INTO audit_log (timestamp, event_type, actor, data, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![timestamp, event_type, actor, data, prev_hash, hash],
    )?;

    Ok(())
}

/// Records one tool-call policy decision (allow/deny/confirm) into the audit
/// ledger with a normalized JSON envelope.
///
/// Raw arguments are never persisted — callers pass a pre-computed hash — so
/// the ledger cannot itself become a secret-leak vector. Shared by the MCP
/// (`src/mcp/tools.rs`) and daemon WS (`src/daemon/handlers.rs`) dispatch
/// paths so both feed the same `raios policy suggest` learning pipeline
/// (Phase 1: reads `event_type`/`data` back out to propose new
/// `[[tools.rules]]` entries).
pub fn record_tool_decision(
    conn: &Connection,
    tool: &str,
    args_hash: &str,
    matched_rule: &str,
    event_type: &str,
    actor: &str,
) -> Result<()> {
    let data = json!({
        "tool": tool,
        "args_hash": args_hash,
        "matched_rule": matched_rule,
    })
    .to_string();
    record_audit_event(conn, event_type, actor, &data)
}

/// Verify the integrity of the entire audit_log chain.
///
/// Returns `Ok(n)` where n is the number of entries verified,
/// or `Err(...)` describing the first broken link found.
pub fn verify_chain(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, event_type, actor, data, prev_hash, hash
         FROM audit_log ORDER BY id ASC",
    )?;

    struct Row {
        id: i64,
        timestamp: String,
        event_type: String,
        actor: String,
        data: String,
        prev_hash: String,
        hash: String,
    }

    let rows: Vec<Row> = stmt
        .query_map([], |row| {
            Ok(Row {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                event_type: row.get(2)?,
                actor: row.get(3)?,
                data: row.get(4)?,
                prev_hash: row.get(5)?,
                hash: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;

    for (i, row) in rows.iter().enumerate() {
        // Verify the hash recorded matches what we'd compute
        let expected = compute_hash(
            &row.prev_hash,
            &row.timestamp,
            &row.event_type,
            &row.actor,
            &row.data,
        );
        if expected != row.hash {
            return Err(anyhow!(
                "Chain broken at entry id={}: computed hash {} != stored hash {}",
                row.id,
                expected,
                row.hash
            ));
        }

        // Verify linkage: prev_hash of entry[n] must equal hash of entry[n-1]
        if i > 0 {
            let prev_row = &rows[i - 1];
            if row.prev_hash != prev_row.hash {
                return Err(anyhow!(
                    "Chain broken at entry id={}: prev_hash does not match hash of entry id={}",
                    row.id,
                    prev_row.id
                ));
            }
        }
    }

    Ok(rows.len())
}

// ─── Internal ─────────────────────────────────────────────────────────────────

fn compute_hash(
    prev_hash: &str,
    timestamp: &str,
    event_type: &str,
    actor: &str,
    data: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(prev_hash.as_bytes());
    h.update(b"|");
    h.update(timestamp.as_bytes());
    h.update(b"|");
    h.update(event_type.as_bytes());
    h.update(b"|");
    h.update(actor.as_bytes());
    h.update(b"|");
    h.update(data.as_bytes());
    format!("{:x}", h.finalize())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE audit_log (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp  TEXT NOT NULL,
                event_type TEXT NOT NULL,
                actor      TEXT NOT NULL DEFAULT 'raios',
                data       TEXT NOT NULL DEFAULT '',
                prev_hash  TEXT NOT NULL DEFAULT '',
                hash       TEXT NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn empty_chain_verifies_ok() {
        let conn = in_memory_db();
        assert_eq!(verify_chain(&conn).unwrap(), 0);
    }

    #[test]
    fn chain_verifies_after_multiple_events() {
        let conn = in_memory_db();
        record_audit_event(&conn, "tool_call", "raios", "list_projects").unwrap();
        record_audit_event(&conn, "policy_deny", "raios", "run_build blocked").unwrap();
        record_audit_event(&conn, "path_blocked", "raios", "traversal attempt").unwrap();
        assert_eq!(verify_chain(&conn).unwrap(), 3);
    }

    #[test]
    fn tampered_hash_is_detected() {
        let conn = in_memory_db();
        record_audit_event(&conn, "tool_call", "raios", "initial entry").unwrap();
        // Directly tamper with the stored hash
        conn.execute(
            "UPDATE audit_log SET hash = 'aaabbbccc000' WHERE id = 1",
            [],
        )
        .unwrap();
        let result = verify_chain(&conn);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Chain broken"));
    }

    #[test]
    fn tool_decision_writes_normalized_envelope_and_chains() {
        let conn = in_memory_db();
        record_tool_decision(&conn, "run_build", "deadbeef", "rule", "tool_allow", "claude_kaira")
            .unwrap();
        record_tool_decision(&conn, "git_commit", "cafebabe", "default", "tool_confirm", "codex_kaira")
            .unwrap();

        // Chain integrity holds across mixed record_audit_event / record_tool_decision writers.
        assert_eq!(verify_chain(&conn).unwrap(), 2);

        let (event_type, actor, data): (String, String, String) = conn
            .query_row(
                "SELECT event_type, actor, data FROM audit_log WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(event_type, "tool_allow");
        assert_eq!(actor, "claude_kaira");
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(parsed["tool"], "run_build");
        assert_eq!(parsed["args_hash"], "deadbeef");
        assert_eq!(parsed["matched_rule"], "rule");
        // Raw arguments must never appear in the ledger — only the hash.
        assert!(!data.contains("--force"));
    }

    #[test]
    fn broken_link_between_entries_detected() {
        let conn = in_memory_db();
        record_audit_event(&conn, "event_a", "raios", "first").unwrap();
        record_audit_event(&conn, "event_b", "raios", "second").unwrap();
        // Tamper with prev_hash of the second entry
        conn.execute(
            "UPDATE audit_log SET prev_hash = 'deadbeef' WHERE id = 2",
            [],
        )
        .unwrap();
        let result = verify_chain(&conn);
        assert!(result.is_err());
    }
}
