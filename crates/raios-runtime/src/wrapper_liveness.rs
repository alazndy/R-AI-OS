//! Conservative wrapper recovery. Process identity is authoritative; elapsed
//! time and inaccessible process namespaces never prove an agent is dead.
use anyhow::{Context, Result};
use raios_core::db::{cp_finish_wrapper_session, WrapperProcessIdentity};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SessionRecoveryCandidate {
    pub run_id: String,
    pub task_id: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct SessionRecoveryReport {
    pub applied: bool,
    pub process_view_available: bool,
    pub include_legacy: bool,
    pub inspected: usize,
    pub kept_alive_or_unverified: usize,
    pub candidates: Vec<SessionRecoveryCandidate>,
    pub closed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcessState {
    Alive,
    Dead,
    Unverified,
}

struct ProcessView {
    host_id: String,
    boot_id: String,
    pid_namespace: String,
    boot_local: Option<chrono::NaiveDateTime>,
}

impl ProcessView {
    #[cfg(target_os = "linux")]
    fn current() -> Option<Self> {
        let read = |p| {
            std::fs::read_to_string(p)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let pid = std::process::id();
        let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
        // Check that this /proc actually describes our process namespace.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_stat(&stat, pid)?;
        let boot_seconds = std::fs::read_to_string("/proc/stat")
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("btime ")?.parse::<i64>().ok())?;
        Some(Self {
            host_id: read("/etc/machine-id")?,
            boot_id: read("/proc/sys/kernel/random/boot_id")?,
            pid_namespace: namespace.to_string_lossy().into_owned(),
            boot_local: chrono::DateTime::from_timestamp(boot_seconds, 0)
                .map(|v| v.with_timezone(&chrono::Local).naive_local()),
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn current() -> Option<Self> {
        None
    }

    fn identity(&self, pid: u32) -> std::io::Result<WrapperProcessIdentity> {
        let body = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let (start_ticks, _) = parse_stat(&body, pid).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid process stat")
        })?;
        Ok(WrapperProcessIdentity {
            host_id: self.host_id.clone(),
            boot_id: self.boot_id.clone(),
            pid_namespace: self.pid_namespace.clone(),
            pid,
            start_ticks,
        })
    }

    fn state(&self, identity: &WrapperProcessIdentity) -> ProcessState {
        if identity.host_id != self.host_id {
            return ProcessState::Unverified;
        }
        if identity.boot_id != self.boot_id {
            return ProcessState::Dead;
        }
        if identity.pid_namespace != self.pid_namespace {
            return ProcessState::Unverified;
        }
        match std::fs::read_to_string(format!("/proc/{}/stat", identity.pid)) {
            Ok(body) => match parse_stat(&body, identity.pid) {
                Some((ticks, state))
                    if ticks != identity.start_ticks || matches!(state, 'Z' | 'X') =>
                {
                    ProcessState::Dead
                }
                Some(_) => ProcessState::Alive,
                None => ProcessState::Unverified,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ProcessState::Dead,
            Err(_) => ProcessState::Unverified,
        }
    }
}

// comm may itself contain spaces and parentheses; split after its LAST ')'.
fn parse_stat(body: &str, pid: u32) -> Option<(u64, char)> {
    let (prefix, fields) = body.rsplit_once(')')?;
    let (number, _) = prefix.split_once('(')?;
    if number.trim().parse::<u32>().ok()? != pid {
        return None;
    }
    let fields: Vec<_> = fields.split_whitespace().collect();
    Some((
        fields.get(19)?.parse().ok()?,
        fields.first()?.chars().next()?,
    ))
}

pub fn current_wrapper_identity() -> Option<WrapperProcessIdentity> {
    ProcessView::current()?.identity(std::process::id()).ok()
}

pub fn child_identity(pid: u32) -> Option<WrapperProcessIdentity> {
    ProcessView::current()?.identity(pid).ok()
}

fn dead_reason(
    view: &ProcessView,
    owner: &WrapperProcessIdentity,
    child: Option<&WrapperProcessIdentity>,
    child_attached: bool,
    mut probe: impl FnMut(&WrapperProcessIdentity) -> ProcessState,
) -> Option<&'static str> {
    if owner.host_id != view.host_id {
        return None;
    }
    if owner.boot_id != view.boot_id {
        return Some("previous_boot");
    }
    if owner.pid_namespace != view.pid_namespace {
        return None;
    }
    if probe(owner) != ProcessState::Dead {
        return None;
    }
    // A wrapper can die after spawn but before persisting its child. Neither
    // that gap nor an uninspectable/still-live child permits automatic closure.
    if !child_attached {
        return None;
    }
    let child = child?;
    if probe(child) != ProcessState::Dead {
        return None;
    }
    Some("process_disappeared")
}

/// The default is a read-only preview. Automatic daemon calls always exclude
/// legacy rows; the owner reviews those separately before explicit --apply.
pub fn reconcile_sessions(
    path: &Path,
    apply: bool,
    include_legacy: bool,
) -> Result<SessionRecoveryReport> {
    let mut conn = Connection::open_with_flags(
        path,
        if apply {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        },
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    if apply {
        conn.pragma_update(None, "foreign_keys", true)?;
    }
    let tx = conn.transaction_with_behavior(if apply {
        TransactionBehavior::Immediate
    } else {
        TransactionBehavior::Deferred
    })?;
    let has_table: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cp_wrapper_processes')", [], |r| r.get(0))?;
    let sql = if has_table {
        "SELECT r.id,r.task_id,r.started_at,p.owner_json,p.child_json,p.child_attached FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id LEFT JOIN cp_wrapper_processes p ON p.agent_run_id=r.id WHERE r.provider='wrapper' AND r.status='running' ORDER BY r.id"
    } else {
        "SELECT r.id,r.task_id,r.started_at,NULL,NULL,NULL FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id WHERE r.provider='wrapper' AND r.status='running' ORDER BY r.id"
    };
    let view = ProcessView::current();
    let mut report = SessionRecoveryReport {
        applied: apply,
        process_view_available: view.is_some(),
        include_legacy,
        inspected: 0,
        kept_alive_or_unverified: 0,
        candidates: vec![],
        closed: 0,
    };
    if let Some(view) = view {
        let mut stmt = tx.prepare(sql)?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<bool>>(5)?,
            ))
        })?;
        for row in rows {
            let (run_id, task_id, started_at, owner, child, attached) = row?;
            report.inspected += 1;
            let reason = if let Some(owner) = owner {
                let owner: WrapperProcessIdentity =
                    serde_json::from_str(&owner).context("invalid wrapper owner witness")?;
                let child: Option<WrapperProcessIdentity> = child
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .context("invalid wrapper child witness")?;
                dead_reason(
                    &view,
                    &owner,
                    child.as_ref(),
                    attached.unwrap_or(false),
                    |p| view.state(p),
                )
            } else if include_legacy {
                // Legacy timestamps were written in local time; strict parsing
                // and a full-day margin avoid ambiguous DST/clock boundaries.
                let old = started_at.as_deref().and_then(|v| {
                    chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M:%S").ok()
                });
                match (old, view.boot_local) {
                    (Some(old), Some(boot)) if old < boot - chrono::Duration::hours(24) => {
                        Some("legacy_previous_boot")
                    }
                    _ => None,
                }
            } else {
                None
            };
            if let Some(reason) = reason {
                report.candidates.push(SessionRecoveryCandidate {
                    run_id,
                    task_id,
                    reason: reason.into(),
                });
            } else {
                report.kept_alive_or_unverified += 1;
            }
        }
    } else {
        report.inspected = tx.query_row("SELECT count(*) FROM cp_agent_runs r JOIN cp_tasks t ON t.id=r.task_id WHERE r.provider='wrapper' AND r.status='running'", [], |r| r.get(0))?;
        report.kept_alive_or_unverified = report.inspected;
    }
    if apply {
        for candidate in &report.candidates {
            if cp_finish_wrapper_session(
                &tx,
                &candidate.task_id,
                &candidate.run_id,
                false,
                None,
                &candidate.reason,
            )? {
                report.closed += 1;
            }
        }
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_preview_never_writes_and_apply_leaves_legacy_sessions_untouched() {
        let Some(view) = ProcessView::current() else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("db.sqlite");
        let conn = Connection::open(&path).unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let mut owner = view.identity(std::process::id()).unwrap();
        owner.boot_id = "previous-boot".into();
        let (_, dead) = raios_core::db::cp_session_start_in_workspace(
            &conn,
            "codex_kaira",
            None,
            "/workspace",
            Some(&owner),
        )
        .unwrap();
        let (_, legacy) = raios_core::db::cp_session_start(&conn, "codex_kaira", None).unwrap();
        let report = reconcile_sessions(&path, false, false).unwrap();
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.closed, 0);
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_agent_runs WHERE id=?1",
                [&dead],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "running"
        );
        conn.execute_batch("CREATE TRIGGER block_recovery BEFORE UPDATE ON cp_tasks BEGIN SELECT RAISE(ABORT,'fixture failure'); END").unwrap();
        assert!(reconcile_sessions(&path, true, false).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_agent_runs WHERE id=?1",
                [&dead],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "running"
        );
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 0);
        conn.execute_batch("DROP TRIGGER block_recovery").unwrap();
        assert_eq!(reconcile_sessions(&path, true, false).unwrap().closed, 1);
        assert_eq!(
            conn.query_row(
                "SELECT status FROM cp_agent_runs WHERE id=?1",
                [&legacy],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "running"
        );
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
        assert_eq!(reconcile_sessions(&path, true, false).unwrap().closed, 0);
    }
    fn view() -> ProcessView {
        ProcessView {
            host_id: "host".into(),
            boot_id: "boot".into(),
            pid_namespace: "ns".into(),
            boot_local: None,
        }
    }
    fn identity(pid: u32) -> WrapperProcessIdentity {
        WrapperProcessIdentity {
            host_id: "host".into(),
            boot_id: "boot".into(),
            pid_namespace: "ns".into(),
            pid,
            start_ticks: 123,
        }
    }

    #[test]
    fn live_child_or_uncertain_spawn_gap_preserves_the_session() {
        let owner = identity(10);
        let child = identity(11);
        assert_eq!(
            dead_reason(&view(), &owner, Some(&child), true, |p| if p.pid == 10 {
                ProcessState::Dead
            } else {
                ProcessState::Alive
            }),
            None
        );
        assert_eq!(
            dead_reason(&view(), &owner, None, false, |_| ProcessState::Dead),
            None
        );
        assert_eq!(
            dead_reason(&view(), &owner, None, true, |_| ProcessState::Dead),
            None
        );
        assert_eq!(
            dead_reason(&view(), &owner, Some(&child), true, |_| {
                ProcessState::Unverified
            }),
            None
        );
        assert_eq!(
            dead_reason(&view(), &owner, Some(&child), true, |_| ProcessState::Dead),
            Some("process_disappeared")
        );
    }

    #[test]
    fn previous_boot_is_dead_but_another_host_or_namespace_is_unverified() {
        let mut owner = identity(10);
        owner.boot_id = "old".into();
        assert_eq!(
            dead_reason(&view(), &owner, None, false, |_| panic!(
                "previous boot needs no PID probe"
            )),
            Some("previous_boot")
        );
        owner.host_id = "another".into();
        assert_eq!(
            dead_reason(&view(), &owner, None, false, |_| ProcessState::Dead),
            None
        );
        owner = identity(10);
        owner.pid_namespace = "hidden".into();
        assert_eq!(
            dead_reason(&view(), &owner, Some(&identity(11)), true, |_| {
                ProcessState::Dead
            }),
            None
        );
    }

    #[test]
    fn process_stat_handles_parentheses_and_validates_pid() {
        let body = format!(
            "42 (command (with) spaces) S {} 987 0",
            vec!["0"; 18].join(" ")
        );
        assert_eq!(parse_stat(&body, 42), Some((987, 'S')));
        assert_eq!(parse_stat(&body, 41), None);
        assert_eq!(parse_stat("42 (truncated) S", 42), None);
    }

    #[test]
    fn same_pid_with_another_start_time_does_not_identify_the_old_process() {
        if let Some(view) = ProcessView::current() {
            let mut owner = view.identity(std::process::id()).unwrap();
            assert_eq!(view.state(&owner), ProcessState::Alive);
            owner.start_ticks += 1;
            assert_eq!(view.state(&owner), ProcessState::Dead);
        }
    }
}
