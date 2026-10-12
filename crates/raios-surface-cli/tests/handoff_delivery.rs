#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    db: rusqlite::Connection,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        // macOS temporary directories live behind a symlink (/var -> /private/var) and the
        // child process reports its working directory resolved, so the handoff is queued under
        // the resolved path: delivery is scoped to the exact project path.
        let root = dir.path().canonicalize().unwrap();
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        let db = rusqlite::Connection::open(root.join("workspace.db")).unwrap();
        raios_core::db::migrate_existing(&db).unwrap();
        Self {
            _dir: dir,
            root,
            project,
            db,
        }
    }

    fn fake_claude(&self) {
        let path = self.root.join("bin/claude");
        fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$RAIOS_TEST_ARGUMENTS\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn queue(&self, path: &std::path::Path, message: &str) {
        raios_core::db::create_handoff_workflow(
            &self.db,
            raios_core::db::HandoffWorkflowInput {
                project_path: path.to_str().unwrap(),
                from_agent: "codex_kaira",
                to_agent: "claude_kaira",
                status: "SUCCESS",
                msg: message,
                diff_stat: None,
                report: None,
            },
        )
        .unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_raios"))
            .args(args)
            .current_dir(&self.project)
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("RAIOS_DB_PATH", self.root.join("workspace.db"))
            .env("PATH", self.root.join("bin"))
            .env("RAIOS_TEST_ARGUMENTS", self.root.join("arguments"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Keep stdin open and empty: the old Enter pause would block forever.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "handoff blocked on empty stdin: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        child.wait_with_output().unwrap()
    }
}

#[test]
fn cwd_handoff_reaches_claude_without_enter_and_leaves_other_projects_pending() {
    let f = Fixture::new();
    f.fake_claude();
    f.queue(&f.project, "review project A");
    f.queue(&f.root.join("other-project"), "review project B");
    let output = f.run(&["run", "claude"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let args = fs::read_to_string(f.root.join("arguments")).unwrap();
    assert!(args.contains("--append-system-prompt"));
    assert!(args.contains("review project A"));
    assert!(!args.contains("review project B"));
    assert!(!args.contains("--print"));
    assert!(raios_core::db::cp_take_pending_handoff_for_path(
        &f.db,
        f.project.to_str().unwrap(),
        "claude_kaira"
    )
    .unwrap()
    .is_none());
    assert!(raios_core::db::cp_take_pending_handoff_for_path(
        &f.db,
        f.root.join("other-project").to_str().unwrap(),
        "claude_kaira"
    )
    .unwrap()
    .is_some());
}

#[test]
fn handoff_run_records_and_starts_the_receiver() {
    let f = Fixture::new();
    f.fake_claude();
    let output = f.run(&[
        "handoff",
        "--to",
        "claude-kaira",
        "--status",
        "success",
        "--msg",
        "continue review",
        "--run",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::read_to_string(f.root.join("arguments"))
        .unwrap()
        .contains("continue review"));
    let count: i64 =
        f.db.query_row(
            "SELECT count(*) FROM cp_approvals WHERE status='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn failed_spawn_keeps_handoff_pending_and_closes_the_wrapper_session() {
    let f = Fixture::new();
    f.queue(&f.project, "review changes");
    let output = f.run(&["run", "claude"]);
    assert!(!output.status.success());
    assert!(raios_core::db::cp_take_pending_handoff_for_path(
        &f.db,
        f.project.to_str().unwrap(),
        "claude_kaira"
    )
    .unwrap()
    .is_some());
    let count: i64 =
        f.db.query_row(
            "SELECT count(*) FROM cp_agent_runs WHERE status='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn json_before_handoff_rejects_run_before_any_state_write() {
    let f = Fixture::new();
    let output = f.run(&[
        "--json",
        "handoff",
        "--to",
        "claude-kaira",
        "--status",
        "success",
        "--msg",
        "ready",
        "--run",
    ]);
    assert!(!output.status.success());
    let count: i64 =
        f.db.query_row("SELECT count(*) FROM cp_tasks", [], |r| r.get(0))
            .unwrap();
    assert_eq!(count, 0);
}
