use std::cell::Cell;

use raios_contracts::{Command, Problem};
use raios_core::security::PolicyConfig;
use rusqlite::{params, Connection};
use serde_json::Value;

use crate::control_plane::service::{dispatch_with_seams, ControlActor};

const PROJECT: &str = "/workspace/registered-app";

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    raios_core::db::migrate_existing(&conn).unwrap();
    conn.execute(
        "INSERT INTO projects (name, path) VALUES ('registered-app', ?1)",
        params![PROJECT],
    )
    .unwrap();
    conn
}

fn validate(key: &str, agent: &str, path: &str, prompt: Option<&str>) -> Command {
    Command::ValidateAgentLaunch {
        agent_name: agent.into(),
        project_path: path.into(),
        prompt: prompt.map(str::to_owned),
        idempotency_key: key.into(),
    }
}

fn launch(key: &str, agent: &str, path: &str, prompt: Option<&str>) -> Command {
    Command::LaunchAgent {
        agent_name: agent.into(),
        project_path: path.into(),
        prompt: prompt.map(str::to_owned),
        idempotency_key: key.into(),
    }
}

/// Runs `command` with a launcher that only counts how often it is called, and no policy file:
/// the dispatcher's default (`confirm`) applies, whatever the machine running the test holds.
fn run(
    conn: &mut Connection,
    actor: &ControlActor,
    command: &Command,
    launches: &Cell<u32>,
) -> Result<Option<Value>, Problem> {
    run_with_policy(conn, actor, command, launches, &|| None)
}

/// A policy whose only rule sets `launch_agent` to `action`, parsed from a temporary file.
fn policy_with_launch_agent(action: &str) -> PolicyConfig {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("raios-policy.toml");
    std::fs::write(
        &path,
        format!(
            "[server]\nhttp_port = 42071\n\n[filesystem]\nenforce_sandbox = true\n\
             allowed_paths = []\nblocked_paths = []\n\n[tools]\n\
             default_action = \"confirm\"\n\n[[tools.rules]]\nname = \"launch_agent\"\n\
             action = \"{action}\"\n"
        ),
    )
    .unwrap();
    PolicyConfig::load_from_file(&path).unwrap()
}

fn run_with_policy(
    conn: &mut Connection,
    actor: &ControlActor,
    command: &Command,
    launches: &Cell<u32>,
    load_policy: &dyn Fn() -> Option<PolicyConfig>,
) -> Result<Option<Value>, Problem> {
    dispatch_with_seams(
        conn,
        actor,
        command,
        &|_request| {
            launches.set(launches.get() + 1);
            Ok(())
        },
        load_policy,
    )
}

fn row_count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn validating_a_launch_reports_the_normalized_agent_and_starts_nothing() {
    let mut conn = db();
    let launches = Cell::new(0);

    let result = run(
        &mut conn,
        &ControlActor::local_session(),
        &validate("v-1", " AGY ", PROJECT, Some("  inspect the build ")),
        &launches,
    )
    .unwrap()
    .unwrap();

    assert_eq!(result["status"], "validated");
    assert_eq!(result["agent_name"], "antigravity");
    assert_eq!(result["project_path"], PROJECT);
    assert_eq!(launches.get(), 0, "the daemon must not start anything");
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
    let (command_type, status): (String, String) = conn
        .query_row(
            "SELECT command_type, status FROM cp_idempotency WHERE idempotency_key = 'v-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(command_type, "validate_agent_launch");
    assert_eq!(status, "COMPLETED");
}

#[test]
fn launch_agent_still_starts_the_launcher_exactly_once() {
    // Control for the test above: the same harness does see a launch when one happens.
    let mut conn = db();
    let launches = Cell::new(0);

    let result = run(
        &mut conn,
        &ControlActor::local_session(),
        &launch("l-1", "claude", PROJECT, None),
        &launches,
    )
    .unwrap()
    .unwrap();

    assert_eq!(result["status"], "launching");
    assert_eq!(result["agent_name"], "claude");
    assert_eq!(launches.get(), 1);
}

#[test]
fn both_commands_reject_the_same_bad_input_with_the_same_code() {
    let long_prompt = "x".repeat(601);
    let cases: [(&str, &str, Option<&str>, &str); 7] = [
        ("codex; touch /tmp/pwned", PROJECT, None, "INVALID_INPUT"),
        ("claude", "relative/path", None, "INVALID_INPUT"),
        ("claude", "/workspace/not-registered", None, "NOT_FOUND"),
        ("claude", PROJECT, Some("--dangerous-flag"), "INVALID_INPUT"),
        (
            "claude",
            PROJECT,
            Some(long_prompt.as_str()),
            "INVALID_INPUT",
        ),
        (
            "claude",
            PROJECT,
            Some("use key sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
            "INVALID_INPUT",
        ),
        ("claude", "", None, "INVALID_INPUT"),
    ];
    for (index, (agent, path, prompt, code)) in cases.into_iter().enumerate() {
        for (label, command) in [
            (
                "validate",
                validate(&format!("v-{index}"), agent, path, prompt),
            ),
            ("launch", launch(&format!("l-{index}"), agent, path, prompt)),
        ] {
            let mut conn = db();
            let launches = Cell::new(0);
            let error = run(
                &mut conn,
                &ControlActor::local_session(),
                &command,
                &launches,
            )
            .unwrap_err();
            assert_eq!(error.code, code, "{label} {agent:?} {path:?} {prompt:?}");
            assert_eq!(launches.get(), 0);
            assert_eq!(
                row_count(&conn, "cp_idempotency"),
                0,
                "rejected input leaves no trace"
            );
        }
    }
}

#[test]
fn a_retry_with_the_same_key_returns_the_first_result() {
    let mut conn = db();
    let launches = Cell::new(0);
    let command = validate("v-retry", "opencode", PROJECT, None);
    let actor = ControlActor::local_session();

    let first = run(&mut conn, &actor, &command, &launches).unwrap();
    let second = run(&mut conn, &actor, &command, &launches).unwrap();

    assert_eq!(first, second);
    assert_eq!(row_count(&conn, "cp_idempotency"), 1);
    assert_eq!(raios_core::security::verify_chain(&conn).unwrap(), 1);
}

#[test]
fn the_same_key_with_a_different_payload_is_a_collision() {
    let mut conn = db();
    let launches = Cell::new(0);
    let actor = ControlActor::local_session();
    run(
        &mut conn,
        &actor,
        &validate("v-same", "opencode", PROJECT, None),
        &launches,
    )
    .unwrap();

    let error = run(
        &mut conn,
        &actor,
        &validate("v-same", "codex", PROJECT, None),
        &launches,
    )
    .unwrap_err();

    assert_eq!(error.code, "INVALID_INPUT");
    assert!(error.message.contains("Idempotency collision"));
}

#[test]
fn a_principal_that_cannot_mutate_is_refused() {
    let mut conn = db();
    let launches = Cell::new(0);

    let error = run(
        &mut conn,
        &ControlActor::remote_session("someone-else"),
        &validate("v-remote", "claude", PROJECT, None),
        &launches,
    )
    .unwrap_err();

    assert_eq!(error.code, "UNAUTHORIZED");
}

#[test]
fn denying_launch_agent_in_the_policy_also_denies_validating_a_launch() {
    let mut conn = db();
    let launches = Cell::new(0);

    let error = run_with_policy(
        &mut conn,
        &ControlActor::local_session(),
        &validate("v-deny", "claude", PROJECT, None),
        &launches,
        &|| Some(policy_with_launch_agent("deny")),
    )
    .unwrap_err();

    assert_eq!(error.code, "FORBIDDEN");
    assert_eq!(launches.get(), 0);
    assert_eq!(row_count(&conn, "cp_idempotency"), 0);
    let denied: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'tool_deny'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(denied, 1, "the denial is audited");
}

#[test]
fn an_allow_rule_for_launch_agent_is_recorded_as_tool_allow_for_validation() {
    let mut conn = db();
    let launches = Cell::new(0);

    run_with_policy(
        &mut conn,
        &ControlActor::local_session(),
        &validate("v-allow", "claude", PROJECT, None),
        &launches,
        &|| Some(policy_with_launch_agent("allow")),
    )
    .unwrap();

    let allowed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = 'tool_allow'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(allowed, 1);
}
