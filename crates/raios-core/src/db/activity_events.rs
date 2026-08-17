//! Read/write layer for `activity_events` and `notification_cursors`.
//!
//! `activity_events` is an append-only log written by background producers
//! (lifecycle, git, budget, etc. — Tasks 5-9). `notification_cursors` tracks,
//! per client, how far each client has consumed the "important" stream and
//! when it last received a "routine" digest window. This module only reads
//! and writes those two tables; it does not decide what counts as important
//! vs routine (that's the caller's `tier` argument) and does not know about
//! HTTP, daemons, or any specific client (Task 11 and beyond).

use rusqlite::{params, Connection, OptionalExtension};

const EPOCH_TS: &str = "1970-01-01T00:00:00Z";
const TS_FMT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Append one activity event row. `tier` must be `"important"` or
/// `"routine"` — enforced by the `activity_events.tier` CHECK constraint in
/// the schema, not re-validated here.
pub fn log_activity_event(
    conn: &Connection,
    source: &str,
    project: Option<&str>,
    tier: &str,
    summary: &str,
    detail_json: Option<&str>,
) -> rusqlite::Result<()> {
    let ts = chrono::Utc::now().format(TS_FMT).to_string();
    conn.execute(
        "INSERT INTO activity_events (ts, source, project, tier, summary, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts, source, project, tier, summary, detail_json],
    )?;
    Ok(())
}

/// One row from `activity_events`, projected for client consumption.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ActivityEvent {
    pub ts: String,
    pub source: String,
    pub project: Option<String>,
    pub summary: String,
}

/// A batch of `"routine"`-tier events accumulated between two digest polls.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DigestWindow {
    pub since_ts: String,
    pub until_ts: String,
    pub events: Vec<ActivityEvent>,
}

/// Fetch the client's cursor row, creating a default (epoch) one if this is
/// the first time `client_id` has been seen.
fn ensure_cursor(conn: &Connection, client_id: &str) -> rusqlite::Result<(String, String)> {
    let existing = conn
        .query_row(
            "SELECT last_important_ts, last_digest_ts FROM notification_cursors WHERE client_id = ?1",
            params![client_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;

    if let Some(cursor) = existing {
        return Ok(cursor);
    }

    conn.execute(
        "INSERT INTO notification_cursors (client_id) VALUES (?1)",
        params![client_id],
    )?;
    Ok((EPOCH_TS.to_string(), EPOCH_TS.to_string()))
}

/// Return `"important"`-tier events newer than `client_id`'s cursor, then
/// advance the cursor to the latest returned `ts`. Idempotent when there is
/// nothing new: the cursor is left untouched, so a caller can poll on an
/// interval without missing or duplicating events.
pub fn poll_important_events(
    conn: &Connection,
    client_id: &str,
) -> rusqlite::Result<Vec<ActivityEvent>> {
    let (last_important_ts, _) = ensure_cursor(conn, client_id)?;

    let mut stmt = conn.prepare(
        "SELECT ts, source, project, summary FROM activity_events
         WHERE tier = 'important' AND ts > ?1 ORDER BY ts ASC",
    )?;
    let events: Vec<ActivityEvent> = stmt
        .query_map(params![last_important_ts], |row| {
            Ok(ActivityEvent {
                ts: row.get(0)?,
                source: row.get(1)?,
                project: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    if let Some(last) = events.last() {
        conn.execute(
            "UPDATE notification_cursors SET last_important_ts = ?1 WHERE client_id = ?2",
            params![last.ts.clone(), client_id],
        )?;
    }

    Ok(events)
}

/// If at least `digest_interval_secs` have elapsed since `client_id`'s last
/// digest, return the `"routine"`-tier events accumulated in that window and
/// advance the digest cursor to "now". Otherwise return `None` and leave the
/// cursor untouched.
pub fn poll_digest_window(
    conn: &Connection,
    client_id: &str,
    digest_interval_secs: i64,
) -> rusqlite::Result<Option<DigestWindow>> {
    let (_, last_digest_ts) = ensure_cursor(conn, client_id)?;

    let last_dt = chrono::NaiveDateTime::parse_from_str(&last_digest_ts, TS_FMT)
        .map(|naive| naive.and_utc())
        .unwrap_or_else(|_| chrono::DateTime::from_timestamp(0, 0).unwrap());
    let now = chrono::Utc::now();

    if (now - last_dt).num_seconds() < digest_interval_secs {
        return Ok(None);
    }

    let until_ts = now.format(TS_FMT).to_string();

    let mut stmt = conn.prepare(
        "SELECT ts, source, project, summary FROM activity_events
         WHERE tier = 'routine' AND ts > ?1 AND ts <= ?2 ORDER BY ts ASC",
    )?;
    let events: Vec<ActivityEvent> = stmt
        .query_map(params![last_digest_ts, until_ts], |row| {
            Ok(ActivityEvent {
                ts: row.get(0)?,
                source: row.get(1)?,
                project: row.get(2)?,
                summary: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    conn.execute(
        "UPDATE notification_cursors SET last_digest_ts = ?1 WHERE client_id = ?2",
        params![until_ts.clone(), client_id],
    )?;

    Ok(Some(DigestWindow {
        since_ts: last_digest_ts,
        until_ts,
        events,
    }))
}

/// Delete `activity_events` rows older than `days` days. Returns the number
/// of rows deleted. Used by the retention job (Task 10).
pub fn prune_activity_events_older_than(conn: &Connection, days: i64) -> rusqlite::Result<usize> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
        .format(TS_FMT)
        .to_string();
    conn.execute("DELETE FROM activity_events WHERE ts < ?1", params![cutoff])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn log_activity_event_writes_a_retrievable_row() {
        let conn = test_conn();
        log_activity_event(&conn, "lifecycle", Some("demo"), "important", "demo archived", None)
            .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn poll_important_events_returns_only_new_rows_and_advances_cursor() {
        let conn = test_conn();
        log_activity_event(&conn, "lifecycle", Some("a"), "important", "a archived", None)
            .unwrap();

        let first_poll = poll_important_events(&conn, "raios-tray").unwrap();
        assert_eq!(first_poll.len(), 1);
        assert_eq!(first_poll[0].summary, "a archived");

        let second_poll = poll_important_events(&conn, "raios-tray").unwrap();
        assert!(
            second_poll.is_empty(),
            "cursor must have advanced past the first event"
        );
    }

    #[test]
    fn poll_important_events_cursors_are_isolated_per_client() {
        let conn = test_conn();
        log_activity_event(&conn, "lifecycle", Some("a"), "important", "a archived", None)
            .unwrap();

        let tray_poll = poll_important_events(&conn, "raios-tray").unwrap();
        assert_eq!(tray_poll.len(), 1);

        let gnome_poll = poll_important_events(&conn, "kaira-gnome-ext").unwrap();
        assert_eq!(
            gnome_poll.len(),
            1,
            "a second, distinct client_id must see the event independently"
        );
    }

    #[test]
    fn poll_important_events_ignores_routine_tier_rows() {
        let conn = test_conn();
        log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None)
            .unwrap();

        let events = poll_important_events(&conn, "raios-tray").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn poll_digest_window_returns_none_before_interval_elapses() {
        let conn = test_conn();
        log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None)
            .unwrap();

        // First call with a very long interval — cursor starts at epoch, so "now - epoch"
        // is always >= interval on the *first* call regardless of interval size; to test
        // the "not yet" path we must first advance the cursor via one real call, then
        // immediately call again with a large interval.
        let first = poll_digest_window(&conn, "raios-tray", 0).unwrap();
        assert!(
            first.is_some(),
            "first-ever call with interval=0 must fire immediately"
        );

        let second = poll_digest_window(&conn, "raios-tray", 3600).unwrap();
        assert!(
            second.is_none(),
            "an immediate second call with a 1h interval must not fire again"
        );
    }

    #[test]
    fn poll_digest_window_groups_only_routine_tier_rows_in_the_window() {
        let conn = test_conn();
        log_activity_event(&conn, "git", Some("a"), "routine", "a: branch changed", None)
            .unwrap();
        log_activity_event(&conn, "lifecycle", Some("b"), "important", "b archived", None)
            .unwrap();

        let window = poll_digest_window(&conn, "raios-tray", 0).unwrap().unwrap();
        assert_eq!(window.events.len(), 1);
        assert_eq!(window.events[0].summary, "a: branch changed");
    }

    #[test]
    fn prune_activity_events_older_than_deletes_only_stale_rows() {
        let conn = test_conn();
        conn.execute(
            "INSERT INTO activity_events (ts, source, project, tier, summary)
             VALUES ('2020-01-01T00:00:00Z', 'git', 'old', 'routine', 'stale row')",
            [],
        )
        .unwrap();
        log_activity_event(&conn, "git", Some("new"), "routine", "fresh row", None).unwrap();

        let deleted = prune_activity_events_older_than(&conn, 30).unwrap();
        assert_eq!(deleted, 1);

        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1);
    }
}
