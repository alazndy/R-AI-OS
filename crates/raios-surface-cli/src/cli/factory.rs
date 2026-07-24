//! Local Product Factory CLI boundary.
//!
//! This surface accepts one serialized `FactoryCommand` from a bounded local
//! file. It deliberately reuses the canonical runtime dispatcher so ownership,
//! feature-gating, idempotency, auditing, and secret screening cannot drift
//! from the TUI and MCP paths.

use super::FactoryAction;
use raios_contracts::FactoryCommand;
use rusqlite::Connection;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

const MAX_COMMAND_FILE_BYTES: u64 = 64 * 1024;

pub(super) fn cmd_factory(action: FactoryAction, json: bool) {
    let result = match action {
        FactoryAction::Overview => factory_overview(),
        FactoryAction::Execute { file } => {
            read_factory_command(&file).and_then(execute_factory_command)
        }
    };

    match result {
        Ok(payload) if json => println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
        ),
        Ok(payload) => print_human(&payload),
        Err(error) => {
            if json {
                println!("{}", serde_json::json!({"ok": false, "error": error}));
            } else {
                eprintln!("Factory command failed: {error}");
            }
            std::process::exit(1);
        }
    }
}

fn factory_overview() -> Result<serde_json::Value, String> {
    let conn = raios_core::db::open_db().map_err(|error| error.to_string())?;
    factory_overview_from_conn(&conn)
}

fn factory_overview_from_conn(conn: &Connection) -> Result<serde_json::Value, String> {
    let snapshot = raios_runtime::control_plane::service::load_work_snapshot(conn)
        .map_err(|error| format!("Failed loading Product Factory overview: {error}"))?;
    Ok(serde_json::json!(snapshot.factory))
}

fn read_factory_command(path: &Path) -> Result<FactoryCommand, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Unable to inspect Factory command file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err("Factory command file must be a regular local file".into());
    }
    if metadata.len() > MAX_COMMAND_FILE_BYTES {
        return Err(format!(
            "Factory command file exceeds the {} byte limit",
            MAX_COMMAND_FILE_BYTES
        ));
    }

    let mut contents = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .map_err(|error| format!("Unable to read Factory command file: {error}"))?
        .take(MAX_COMMAND_FILE_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("Unable to read Factory command file: {error}"))?;
    if contents.len() as u64 > MAX_COMMAND_FILE_BYTES {
        return Err(format!(
            "Factory command file exceeds the {} byte limit",
            MAX_COMMAND_FILE_BYTES
        ));
    }

    serde_json::from_slice(&contents)
        .map_err(|error| format!("invalid FactoryCommand envelope: {error}"))
}

fn execute_factory_command(command: FactoryCommand) -> Result<serde_json::Value, String> {
    if !cli_may_execute(&command) {
        return Err(
            "factory_approval_required: this command must be approved by the human owner in the Factory UI"
                .into(),
        );
    }

    let factory_enabled = raios_core::config::Config::load()
        .map(|config| config.factory.enabled)
        .unwrap_or(false);
    let actor = raios_runtime::control_plane::service::ControlActor::local_session();
    let mut conn = raios_core::db::open_db().map_err(|error| error.to_string())?;
    dispatch_with_conn(&mut conn, factory_enabled, &actor, &command)
}

fn dispatch_with_conn(
    conn: &mut Connection,
    factory_enabled: bool,
    actor: &raios_runtime::control_plane::service::ControlActor,
    command: &FactoryCommand,
) -> Result<serde_json::Value, String> {
    raios_runtime::product_factory::dispatch_factory_command(conn, actor, factory_enabled, command)
        .map(|result| serde_json::json!({"result": result}))
        .map_err(|problem| format!("factory_command_failed:{}", problem.message))
}

/// Mirrors the MCP `factory_execute` allow-list. Approval, requirement-apply,
/// cancellation, stage activation/completion, and release approval remain
/// human-only operations in the Factory UI.
fn cli_may_execute(command: &FactoryCommand) -> bool {
    matches!(
        command,
        FactoryCommand::CreateWorkspace { .. }
            | FactoryCommand::CreateProductDraft { .. }
            | FactoryCommand::SetProductMode { .. }
            | FactoryCommand::AttachExistingProject { .. }
            | FactoryCommand::StartIntake { .. }
            | FactoryCommand::RecordIntakeAnswer { .. }
            | FactoryCommand::CreateCharterDraft { .. }
            | FactoryCommand::GenerateCharterDraft { .. }
            | FactoryCommand::CreateRequirementDraft { .. }
            | FactoryCommand::SubmitChangeRequest { .. }
            | FactoryCommand::AssessChangeRequest { .. }
            | FactoryCommand::CreatePlanDraft { .. }
            | FactoryCommand::MaterializePlannedCycle { .. }
            | FactoryCommand::PauseCycle { .. }
            | FactoryCommand::ResumeCycle { .. }
            | FactoryCommand::MaterializeStageTaskGraph { .. }
            | FactoryCommand::RecordStageEvidence { .. }
            | FactoryCommand::LinkStageEvidenceToRequirement { .. }
            | FactoryCommand::InspectReleaseReadiness { .. }
            | FactoryCommand::CreateQualityProfile { .. }
            | FactoryCommand::EnsureReactNativeClosedTestingQualityProfile { .. }
            | FactoryCommand::RecordQualityCheck { .. }
            | FactoryCommand::CreateReleaseDraft { .. }
            | FactoryCommand::CreateSupportItem { .. }
            | FactoryCommand::InspectSupportOverview { .. }
            | FactoryCommand::TriageSupportItem { .. }
            | FactoryCommand::ResolveSupportItem { .. }
            | FactoryCommand::LinkSupportToChangeRequest { .. }
    )
}

fn print_human(payload: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".into())
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use raios_runtime::control_plane::service::ControlActor;
    use rusqlite::Connection;
    use std::io::Write;

    #[test]
    fn overview_is_read_only() {
        let conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let changes_before = total_changes(&conn);

        let overview = factory_overview_from_conn(&conn).unwrap();

        assert!(overview.is_object());
        assert_eq!(total_changes(&conn), changes_before);
    }

    #[test]
    fn malformed_command_file_is_rejected() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{{not valid JSON}}").unwrap();

        let error = read_factory_command(file.path()).unwrap_err();

        assert!(error.starts_with("invalid FactoryCommand envelope:"));
    }

    #[test]
    fn human_approval_commands_are_rejected_before_dispatch() {
        let command = FactoryCommand::ApprovePlan {
            plan_id: "plan-12345".into(),
            idempotency_key: "approve-plan-0001".into(),
        };

        assert!(!cli_may_execute(&command));
    }

    #[test]
    fn allowed_command_reaches_canonical_dispatcher_with_idempotency() {
        let mut conn = Connection::open_in_memory().unwrap();
        raios_core::db::migrate_existing(&conn).unwrap();
        let command = FactoryCommand::CreateWorkspace {
            name: "CLI workspace".into(),
            idempotency_key: "cli-workspace-0001".into(),
        };
        let actor = ControlActor::local_session();

        let first = dispatch_with_conn(&mut conn, true, &actor, &command).unwrap();
        let second = dispatch_with_conn(&mut conn, true, &actor, &command).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM cp_idempotency", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    fn total_changes(conn: &Connection) -> i64 {
        conn.query_row("SELECT total_changes()", [], |row| row.get(0))
            .unwrap()
    }
}
