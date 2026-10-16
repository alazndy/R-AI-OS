//! `rwk_core::usage` against a scripted `raios` CLI.
//!
//! The real CLI reads local logs and auth metadata; these tests script a tiny shell program
//! with the same contract (argv, stdin/stdout/stderr, exit status) so process handling, the
//! time budget and the parsing are all exercised hermetically. The scripts are POSIX shell
//! programs, so the suite is gated to unix — like the search suite it mirrors.
//!
//! The tests run one at a time: `Command::spawn` forks, and a fork that lands while another
//! test's script is between `write` and `exec` makes that exec fail with "Text file busy"
//! (ETXTBSY). The serial lock keeps a scripted spawn and a script write from overlapping.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;
use std::time::Duration;

use rwk_core::error::ErrorCode;
use rwk_core::usage::{usage_with, usage_with_timeout};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Locks the suite and hands out a fresh temp dir for the fake CLI.
fn scripted() -> (MutexGuard<'static, ()>, tempfile::TempDir) {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (serial, tempfile::tempdir().expect("temp dir"))
}

fn fake_cli(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("raios");
    fs::write(&path, body).expect("write the fake CLI");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("make it executable");
    path
}

/// The fake CLI's recorded arguments, one per line.
fn recorded_argv(cli: &Path) -> Vec<String> {
    let recorded = cli.with_extension("argv");
    fs::read_to_string(recorded)
        .expect("the fake CLI records its argv")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_cli_is_called_with_the_documented_arguments() {
    let (_serial, dir) = scripted();
    let cli = fake_cli(
        dir.path(),
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.argv"
printf '[]'
"#,
    );

    let report = usage_with(&cli).expect("usage");
    assert!(report.providers.is_empty());
    assert_eq!(report.quota_line(), None);
    assert_eq!(recorded_argv(&cli), vec!["usage", "--json"]);
}

#[test]
fn remaining_figures_become_the_quota_line_and_null_rows_are_skipped() {
    let (_serial, dir) = scripted();
    let cli = fake_cli(
        dir.path(),
        r#"#!/bin/sh
printf '[{"provider":"Codex / OpenAI","installed":true,"authenticated":true,"remaining":null},{"provider":"Claude Code","installed":true,"authenticated":true,"remaining":"5h:64%% 7d:8%% remaining"}]'
"#,
    );

    let report = usage_with(&cli).expect("usage");
    assert_eq!(report.providers.len(), 2);
    assert_eq!(
        report.quota_line().as_deref(),
        Some("Kota: Claude Code 5h:64% 7d:8% remaining")
    );
}

#[test]
fn a_failing_cli_reports_its_first_stderr_line() {
    let (_serial, dir) = scripted();
    let cli = fake_cli(
        dir.path(),
        "#!/bin/sh\nprintf 'usage failed: provider probe crashed\\n' >&2\nexit 1\n",
    );
    let error = usage_with(&cli).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(
        error.message,
        "Kota okuması başarısız: provider probe crashed"
    );
}

#[test]
fn garbage_output_is_an_internal_error() {
    let (_serial, dir) = scripted();
    let cli = fake_cli(dir.path(), "#!/bin/sh\nprintf 'not json'\n");
    let error = usage_with(&cli).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(error.message.contains("okunamadı"));
}

#[test]
fn a_hung_cli_is_killed_at_the_time_budget() {
    let (_serial, dir) = scripted();
    let cli = fake_cli(dir.path(), "#!/bin/sh\nsleep 30\n");
    let started = std::time::Instant::now();
    let error = usage_with_timeout(&cli, Duration::from_millis(300)).expect_err("must time out");
    assert!(
        error.message.contains("zaman aşımına uğradı"),
        "{}",
        error.message
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "killed in time"
    );
}

#[test]
fn a_missing_program_is_an_io_error() {
    let (_serial, dir) = scripted();
    let missing = dir.path().join("no-such-raios");
    let error = usage_with(&missing).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Io);
    assert!(error.message.contains("başlatılamadı"));
}
