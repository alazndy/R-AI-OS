//! `raios run <agent>` end to end with a fake agent binary: what the wrapper
//! prints, how it records the session, and what it does on a failing agent or
//! a timeout. These pin the behaviour of the whole wrapper so its pieces can
//! be reorganised without changing what users see.

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
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
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
        }
    }

    /// A fake `claude` whose body is the given shell script.
    fn fake_claude(&self, script: &str) {
        let path = self.root.join("bin/claude");
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_raios"))
            .args(args)
            .current_dir(&self.project)
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("RAIOS_DB_PATH", self.root.join("workspace.db"))
            .env("PATH", self.root.join("bin"))
            .env("RAIOS_TEST_SEEN", self.root.join("seen"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "raios run did not finish: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        child.wait_with_output().unwrap()
    }

    /// (status, exit_reason) of the only agent run recorded.
    fn recorded_run(&self) -> (String, Option<String>) {
        let db = rusqlite::Connection::open(self.root.join("workspace.db")).unwrap();
        db.query_row(
            "SELECT status, exit_reason FROM cp_agent_runs ORDER BY rowid DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }
}

/// Stdout with colour codes removed and the volatile bits replaced, so it can
/// be compared line by line: the 8-hex run id and the elapsed seconds.
fn normalized(output: &Output) -> Vec<String> {
    let text = String::from_utf8_lossy(&output.stdout);
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for n in chars.by_ref() {
                if n == 'm' {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    plain
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            // Only the wrapper banner and the run-id lines carry volatile values.
            if line.contains("RAIOS WRAPPER") {
                mask(line, "session: ", "<run>")
            } else if line.contains("Session ended") {
                mask(line, "run: ", "<run>")
            } else if trimmed.starts_with("Duration: ") {
                mask(line, "Duration: ", "<n>s")
            } else if trimmed.starts_with("saved conversation: ") {
                mask(line, "saved conversation: ", "<uuid>")
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// `line` with everything after the first `marker` replaced.
fn mask(line: &str, marker: &str, replacement: &str) -> String {
    match line.find(marker) {
        Some(i) => format!("{}{}", &line[..i + marker.len()], replacement),
        None => line.to_string(),
    }
}

const SUCCESS_SUMMARY: [&str; 7] = [
    "⟦ RAIOS WRAPPER ✓ ⟧  agent: claude_kaira  session: <run>",
    "  saved conversation: <uuid>",
    "✅ Agent (claude) exited: exit status: 0",
    "✓ Session ended  run: <run>",
    "  Duration: <n>s",
    "  Tests in session: no",
    "  Handoff: raios handoff --to claude-kaira --status success --msg \"...\"",
];

#[test]
fn a_successful_run_prints_the_session_summary_and_records_a_clean_exit() {
    let f = Fixture::new();
    f.fake_claude("exit 0");
    let output = f.run(&["run", "claude"]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(normalized(&output), SUCCESS_SUMMARY);
    assert_eq!(
        f.recorded_run(),
        ("succeeded".to_string(), Some("clean_exit".to_string()))
    );
}

#[test]
fn the_agent_sees_its_wrapper_session_and_the_session_block_is_removed_afterwards() {
    let f = Fixture::new();
    // The block is only injected into a CLAUDE.md that already exists.
    fs::create_dir_all(f.root.join("home/.claude")).unwrap();
    fs::write(f.root.join("home/.claude/CLAUDE.md"), "# My instructions\n").unwrap();
    // Record what the child can see while it is running.
    f.fake_claude(
        "{ printf 'RUN=%s\\n' \"$RAIOS_WRAPPER_RUN_ID\"\n\
           printf 'PROJECT=%s\\n' \"$RAIOS_WRAPPER_PROJECT_PATH\"\n\
           printf 'SOCKET=%s\\n' \"$RAIOS_WRAPPER_NOTE_SOCKET\"\n\
           printf 'CLAUDE_MD_HAS_BLOCK=%s\\n' \"$(/usr/bin/grep -c 'RAIOS WRAPPER SESSION' \"$HOME/.claude/CLAUDE.md\" 2>/dev/null)\"\n\
         } > \"$RAIOS_TEST_SEEN\"",
    );
    let output = f.run(&["run", "claude"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let seen = fs::read_to_string(f.root.join("seen")).unwrap();
    let get = |key: &str| {
        seen.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap_or_default()
            .to_string()
    };
    let db = rusqlite::Connection::open(f.root.join("workspace.db")).unwrap();
    let run_id: String = db
        .query_row(
            "SELECT id FROM cp_agent_runs ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(get("RUN"), run_id, "the child gets the recorded run id");
    assert_eq!(get("PROJECT"), f.project.to_str().unwrap());
    assert!(
        get("SOCKET").starts_with("127.0.0.1:"),
        "wrapper-note socket: {seen}"
    );
    assert_eq!(
        get("CLAUDE_MD_HAS_BLOCK"),
        "1",
        "the session block is injected during the run"
    );

    let after = fs::read_to_string(f.root.join("home/.claude/CLAUDE.md")).unwrap();
    assert_eq!(
        after, "# My instructions\n",
        "stripped on exit, original text intact"
    );
}

#[test]
fn a_failing_agent_makes_a_failed_run_and_a_non_zero_exit() {
    let f = Fixture::new();
    f.fake_claude("exit 3");
    let output = f.run(&["run", "claude"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("Agent Runner Error: Agent exited with exit status: 3"));
    // NB: the handoff hint after a failure reads `claude_kaira-kaira` - the
    // identity gets the `-kaira` suffix added a second time. Pinned as-is.
    assert_eq!(
        normalized(&output),
        [
            "⟦ RAIOS WRAPPER ✓ ⟧  agent: claude_kaira  session: <run>",
            "  saved conversation: <uuid>",
            "✅ Agent (claude) exited: exit status: 3",
            "✗ Session ended (non-zero)  run: <run>",
            "  Duration: <n>s",
            "  Tests in session: no",
            "  Handoff: raios handoff --to claude_kaira-kaira --status failed --msg \"...\"",
        ]
    );
    assert_eq!(
        f.recorded_run(),
        ("failed".to_string(), Some("nonzero_exit".to_string()))
    );
}

#[test]
fn the_death_timer_kills_a_slow_agent_and_fails_the_run() {
    let f = Fixture::new();
    f.fake_claude("exec /bin/sleep 30");
    let started = Instant::now();
    let output = f.run(&["run", "claude", "--timeout", "1"]);

    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the agent was not killed"
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Agent claude timed out."));
    assert_eq!(
        normalized(&output),
        [
            "⟦ RAIOS WRAPPER ✓ ⟧  agent: claude_kaira  session: <run>",
            "  saved conversation: <uuid>",
            "⏱️ Death timer active: 1 seconds.",
            "💀 TIMEOUT! Killing agent (claude) for budget safety...",
            "✗ Session ended (non-zero)  run: <run>",
            "  Duration: <n>s",
            "  Tests in session: no",
            "  Handoff: raios handoff --to claude_kaira-kaira --status failed --msg \"...\"",
        ]
    );
    assert_eq!(
        f.recorded_run(),
        ("failed".to_string(), Some("nonzero_exit".to_string()))
    );
}

#[test]
fn an_agent_that_cannot_start_leaves_no_running_session_and_no_session_block() {
    let f = Fixture::new(); // no fake agent on PATH
    let output = f.run(&["run", "claude"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to spawn agent"));
    let db = rusqlite::Connection::open(f.root.join("workspace.db")).unwrap();
    let running: i64 = db
        .query_row(
            "SELECT count(*) FROM cp_agent_runs WHERE status='running'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(running, 0);
    let after = fs::read_to_string(f.root.join("home/.claude/CLAUDE.md")).unwrap_or_default();
    assert!(!after.contains("RAIOS WRAPPER SESSION"));
}
