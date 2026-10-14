//! Handoff commands and receipt queries served by the control plane.

use raios_contracts::{HandoffReceiptDto, Problem};
use rusqlite::Connection;
use serde_json::{json, Value};

use super::service::validate_project_path;

/// Agent identities that may send or receive a handoff.
pub(crate) const HANDOFF_IDENTITIES: &[&str] = &[
    "claude_kaira",
    "codex_kaira",
    "opencode_kaira",
    "antigravity_kaira",
];

/// Upper bound on a handoff summary, in bytes (the same limit workspace 0.1 enforced).
pub(crate) const MAX_HANDOFF_SUMMARY_BYTES: usize = 16_000;

const MAX_APPROVAL_ID_CHARS: usize = 64;

fn validate_identity(raw: &str, role: &str) -> Result<&'static str, Problem> {
    let candidate = raw.trim();
    HANDOFF_IDENTITIES
        .iter()
        .copied()
        .find(|known| *known == candidate)
        .ok_or_else(|| {
            Problem::invalid_input(format!(
                "Handoff {role} must be one of: {}",
                HANDOFF_IDENTITIES.join(", ")
            ))
        })
}

fn validate_summary(raw: &str) -> Result<String, Problem> {
    let summary = raw.trim();
    if summary.is_empty() || summary.len() > MAX_HANDOFF_SUMMARY_BYTES {
        return Err(Problem::invalid_input(format!(
            "Handoff summary must contain 1 to {MAX_HANDOFF_SUMMARY_BYTES} bytes"
        )));
    }
    if raios_core::security::looks_like_secret(summary).is_some() {
        return Err(Problem::invalid_input(
            "Handoff summary appears to contain a secret and was not stored",
        ));
    }
    Ok(summary.to_owned())
}

/// Queues a `BLOCKER` handoff inside the dispatcher's open transaction. The receiving agent
/// has not started; delivery is observed later through [`query_handoff_receipt`].
pub(crate) fn create_handoff(
    tx: &Connection,
    project_path: &str,
    from_agent: &str,
    to_agent: &str,
    summary: &str,
) -> Result<Value, Problem> {
    let project_path = validate_project_path(Some(project_path))?
        .ok_or_else(|| Problem::invalid_input("Project path is required"))?;
    let from_agent = validate_identity(from_agent, "source")?;
    let to_agent = validate_identity(to_agent, "recipient")?;
    if from_agent == to_agent {
        return Err(Problem::invalid_input(
            "Handoff source and recipient must be different agents",
        ));
    }
    let summary = validate_summary(summary)?;

    let ids = raios_core::db::create_handoff_workflow_in_tx(
        tx,
        raios_core::db::HandoffWorkflowInput {
            project_path: &project_path,
            from_agent,
            to_agent,
            status: "BLOCKER",
            msg: &summary,
            diff_stat: None,
            report: None,
        },
    )
    .map_err(|error| Problem::internal(error.to_string()))?;

    Ok(json!({
        "status": "queued",
        "approval_id": ids.approval_id,
        "task_id": ids.task_id,
        "agent_run_id": ids.agent_run_id,
        "artifact_id": ids.artifact_id,
        "project_id": ids.project_id,
        "project_path": project_path,
    }))
}

/// Reads the delivery receipt of one handoff, scoped to `project_path`.
pub(crate) fn query_handoff_receipt(
    conn: &Connection,
    approval_id: &str,
    project_path: &str,
) -> Result<HandoffReceiptDto, Problem> {
    let project_path = validate_project_path(Some(project_path))?
        .ok_or_else(|| Problem::invalid_input("Project path is required"))?;
    let approval_id = approval_id.trim();
    if approval_id.is_empty() || approval_id.chars().count() > MAX_APPROVAL_ID_CHARS {
        return Err(Problem::invalid_input(format!(
            "Approval id must contain 1 to {MAX_APPROVAL_ID_CHARS} characters"
        )));
    }
    let receipt = raios_core::db::cp_handoff_receipt(conn, approval_id, &project_path)
        .map_err(|error| Problem::internal(error.to_string()))?
        .ok_or_else(|| Problem::not_found("Handoff receipt not found for this project"))?;

    let delivered_by_target = receipt.delivered_by_target();
    Ok(HandoffReceiptDto {
        approval_id: receipt.approval_id,
        project_path: receipt.project_path,
        from_agent: receipt.from_agent,
        to_agent: receipt.to_agent,
        status: receipt.status,
        resolved_by: receipt.resolved_by,
        resolved_at: receipt.resolved_at,
        delivered_by_target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::service::{dispatch_control_command, ControlActor};
    use raios_contracts::Command;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        conn
    }

    /// An absolute project path on every platform: a bare `/work/app` is relative on Windows.
    const APP: &str = if cfg!(windows) {
        r"C:\work\app"
    } else {
        "/work/app"
    };

    /// Other absolute project paths, also valid on Windows.
    const NEVER: &str = if cfg!(windows) {
        r"C:\never\registered"
    } else {
        "/never/registered"
    };
    const OTHER: &str = if cfg!(windows) {
        r"C:\other\project"
    } else {
        "/other/project"
    };

    fn handoff(key: &str, project: &str, summary: &str) -> Command {
        Command::CreateHandoff {
            project_path: project.into(),
            from_agent: "claude_kaira".into(),
            to_agent: "codex_kaira".into(),
            summary: summary.into(),
            idempotency_key: key.into(),
        }
    }

    fn run(conn: &mut Connection, command: &Command) -> Result<Option<Value>, Problem> {
        dispatch_control_command(conn, &ControlActor::local_session(), command)
    }

    fn approval_count(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM cp_approvals WHERE approval_type = 'handover'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn queues_a_pending_handoff_and_writes_one_audit_entry() {
        let mut conn = db();
        let result = run(&mut conn, &handoff("h-1", APP, "Continue the migration"))
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "queued");

        let approval_id = result["approval_id"].as_str().unwrap();
        let (status, owner): (String, String) = conn
            .query_row(
                "SELECT status, owner_subject FROM cp_approvals WHERE id = ?1",
                [approval_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (status.as_str(), owner.as_str()),
            ("pending", "local_control_owner")
        );
        assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
    }

    #[test]
    fn works_for_a_project_that_is_not_registered() {
        let mut conn = db();
        let result = run(&mut conn, &handoff("h-2", NEVER, "ctx"))
            .unwrap()
            .unwrap();
        assert!(result["project_id"].is_null());
    }

    #[test]
    fn replaying_the_same_key_returns_the_same_handoff_once() {
        let mut conn = db();
        let first = run(&mut conn, &handoff("h-3", APP, "Same"))
            .unwrap()
            .unwrap();
        let second = run(&mut conn, &handoff("h-3", APP, "Same"))
            .unwrap()
            .unwrap();
        assert_eq!(first["approval_id"], second["approval_id"]);
        assert_eq!(approval_count(&conn), 1);
    }

    #[test]
    fn reusing_a_key_with_a_different_payload_is_rejected() {
        let mut conn = db();
        run(&mut conn, &handoff("h-4", APP, "One")).unwrap();
        let error = run(&mut conn, &handoff("h-4", APP, "Two")).unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT");
        assert!(error.message.contains("Idempotency collision"));
        assert_eq!(approval_count(&conn), 1);
    }

    #[test]
    fn a_remote_principal_cannot_queue_handoffs() {
        let mut conn = db();
        let error = dispatch_control_command(
            &mut conn,
            &ControlActor::remote_session("remote-key"),
            &handoff("h-5", APP, "ctx"),
        )
        .unwrap_err();
        assert_eq!(error.code, "UNAUTHORIZED");
        assert_eq!(approval_count(&conn), 0);
    }

    #[test]
    fn invalid_handoffs_are_rejected_without_writing_anything() {
        let mut conn = db();
        let mut same_agent = handoff("bad-1", APP, "ctx");
        if let Command::CreateHandoff { to_agent, .. } = &mut same_agent {
            *to_agent = "claude_kaira".into();
        }
        let mut unknown_agent = handoff("bad-2", APP, "ctx");
        if let Command::CreateHandoff { from_agent, .. } = &mut unknown_agent {
            *from_agent = "mallory".into();
        }
        let cases = [
            same_agent,
            unknown_agent,
            handoff("bad-3", APP, "   "),
            handoff("bad-4", "relative/path", "ctx"),
            handoff(
                "bad-5",
                APP,
                "here's my key sk-ant-abcdefghijklmnopqrstuvwxyz",
            ),
        ];
        for command in &cases {
            let error = run(&mut conn, command).unwrap_err();
            assert_eq!(error.code, "INVALID_INPUT", "{command:?}");
        }
        assert_eq!(approval_count(&conn), 0);
    }

    #[test]
    fn summary_limit_counts_bytes_not_characters() {
        let mut conn = db();
        // 8,000 two-byte characters are exactly 16,000 bytes and must pass.
        let at_limit = "ğ".repeat(8_000);
        assert_eq!(at_limit.len(), MAX_HANDOFF_SUMMARY_BYTES);
        run(&mut conn, &handoff("limit-1", APP, &at_limit)).unwrap();
        run(
            &mut conn,
            &handoff("limit-2", APP, &"a".repeat(MAX_HANDOFF_SUMMARY_BYTES)),
        )
        .unwrap();

        let over_multibyte = "ğ".repeat(8_001);
        let over_ascii = "a".repeat(MAX_HANDOFF_SUMMARY_BYTES + 1);
        for (key, summary) in [("limit-3", over_multibyte), ("limit-4", over_ascii)] {
            let error = run(&mut conn, &handoff(key, APP, &summary)).unwrap_err();
            assert_eq!(error.code, "INVALID_INPUT");
        }
    }

    #[test]
    fn receipt_tracks_a_handoff_from_pending_to_target_delivery() {
        let mut conn = db();
        let queued = run(&mut conn, &handoff("r-1", APP, "Deliver me"))
            .unwrap()
            .unwrap();
        let approval_id = queued["approval_id"].as_str().unwrap().to_owned();

        let pending = query_handoff_receipt(&conn, &approval_id, APP).unwrap();
        assert_eq!(pending.status, "pending");
        assert_eq!(
            (pending.from_agent.as_str(), pending.to_agent.as_str()),
            ("claude_kaira", "codex_kaira")
        );
        assert!(!pending.delivered_by_target);

        let context = raios_core::db::cp_take_pending_handoff_for_path(&conn, APP, "codex_kaira")
            .unwrap()
            .unwrap();
        raios_core::db::cp_consume_handoff(&conn, &context, "codex_kaira").unwrap();

        let delivered = query_handoff_receipt(&conn, &approval_id, APP).unwrap();
        assert_eq!(delivered.status, "approved");
        assert!(delivered.delivered_by_target);
    }

    #[test]
    fn a_human_approval_is_not_target_delivery() {
        let mut conn = db();
        let queued = run(&mut conn, &handoff("r-2", APP, "ctx"))
            .unwrap()
            .unwrap();
        let approval_id = queued["approval_id"].as_str().unwrap().to_owned();
        run(
            &mut conn,
            &Command::ApproveHandoff {
                approval_id: approval_id.clone(),
                idempotency_key: "r-2-approve".into(),
            },
        )
        .unwrap();

        let receipt = query_handoff_receipt(&conn, &approval_id, APP).unwrap();
        assert_eq!(receipt.status, "approved");
        assert_eq!(receipt.resolved_by.as_deref(), Some("local_control_owner"));
        assert!(!receipt.delivered_by_target);
    }

    #[test]
    fn a_superseded_handoff_reads_as_expired_and_undelivered() {
        let mut conn = db();
        let first = run(&mut conn, &handoff("r-3a", APP, "old"))
            .unwrap()
            .unwrap();
        run(&mut conn, &handoff("r-3b", APP, "new")).unwrap();

        let receipt =
            query_handoff_receipt(&conn, first["approval_id"].as_str().unwrap(), APP).unwrap();
        assert_eq!(receipt.status, "expired");
        assert!(!receipt.delivered_by_target);
    }

    #[test]
    fn receipt_lookup_is_scoped_and_validated() {
        let mut conn = db();
        let queued = run(&mut conn, &handoff("r-4", APP, "ctx"))
            .unwrap()
            .unwrap();
        let approval_id = queued["approval_id"].as_str().unwrap().to_owned();

        let other_project = query_handoff_receipt(&conn, &approval_id, OTHER).unwrap_err();
        assert_eq!(other_project.code, "NOT_FOUND");
        let missing = query_handoff_receipt(&conn, "missing-id", APP).unwrap_err();
        assert_eq!(missing.code, "NOT_FOUND");
        let relative = query_handoff_receipt(&conn, &approval_id, "relative").unwrap_err();
        assert_eq!(relative.code, "INVALID_INPUT");
        let blank = query_handoff_receipt(&conn, "  ", APP).unwrap_err();
        assert_eq!(blank.code, "INVALID_INPUT");
    }
}
