//! `rwk_core::search` against a scripted `raios` CLI.
//!
//! The real CLI needs a daemon and an index; these tests script a tiny shell program with the
//! same contract (argv, stdin/stdout/stderr, exit status) so process handling, the time budget
//! and the parsing are all exercised hermetically. The scripts are POSIX shell programs, so the
//! suite is gated to unix — like the terminal module it stands in for.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rwk_core::error::ErrorCode;
use rwk_core::search::{locate_with, locate_with_timeout};

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
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let cli = fake_cli(
        dir.path(),
        r#"#!/bin/sh
printf '%s\n' "$@" > "$0.argv"
printf '[]'
"#,
    );

    locate_with(&cli, &root, "some pattern", false, 10).expect("search");
    assert_eq!(
        recorded_argv(&cli),
        vec![
            "locate",
            "some pattern",
            "--dir",
            root.to_str().expect("utf-8 root"),
            "-j"
        ]
    );

    locate_with(&cli, &root, "some pattern", true, 10).expect("search");
    assert_eq!(recorded_argv(&cli).last().map(String::as_str), Some("-i"));
}

#[test]
fn hits_come_back_relative_to_the_root_and_the_limit_truncates() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("project");
    fs::create_dir(&root).expect("project dir");
    let cli = fake_cli(
        dir.path(),
        r#"#!/bin/sh
printf '[{"line":1,"path":"%s/src/a.rs","text":"alpha"},{"line":2,"path":"%s/src/b.rs","text":"beta"}]' "$4" "$4"
"#,
    );

    let result = locate_with(&cli, &root, "alpha", false, 1).expect("search");
    assert!(result.truncated);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].path, "src/a.rs");
    assert_eq!(result.hits[0].line, 1);
    assert_eq!(result.hits[0].text, "alpha");

    let result = locate_with(&cli, &root, "alpha", false, 10).expect("search");
    assert!(!result.truncated);
    assert_eq!(
        result
            .hits
            .iter()
            .map(|hit| hit.path.as_str())
            .collect::<Vec<_>>(),
        vec!["src/a.rs", "src/b.rs"]
    );
}

#[test]
fn no_matches_is_an_empty_result() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), "#!/bin/sh\nprintf '[]'\n");
    let result = locate_with(&cli, dir.path(), "nothing", false, 10).expect("search");
    assert!(result.hits.is_empty());
    assert!(!result.truncated);
}

#[test]
fn an_empty_answer_is_an_empty_result() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), "#!/bin/sh\nprintf '  \\n'\n");
    let result = locate_with(&cli, dir.path(), "nothing", false, 10).expect("search");
    assert!(result.hits.is_empty());
    assert!(!result.truncated);
}

#[test]
fn a_failing_cli_reports_its_first_stderr_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(
        dir.path(),
        "#!/bin/sh\nprintf 'locate failed: invalid pattern: boom\\n' >&2\nexit 1\n",
    );
    let error = locate_with(&cli, dir.path(), "boom", false, 10).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Io);
    assert_eq!(error.message, "Arama başarısız: invalid pattern: boom");
}

#[test]
fn garbage_output_is_an_internal_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cli = fake_cli(dir.path(), "#!/bin/sh\nprintf 'not json'\n");
    let error = locate_with(&cli, dir.path(), "boom", false, 10).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Internal);
    assert!(error.message.contains("okunamadı"));
}

#[test]
fn a_hung_cli_is_killed_at_the_time_budget() {
    let dir = tempfile::tempdir().expect("temp dir");
    // Prints nothing and never exits; also the variant that prints and then hangs is covered
    // because the reader only hands over the output once the pipe closes.
    let cli = fake_cli(dir.path(), "#!/bin/sh\nsleep 30\n");
    let started = std::time::Instant::now();
    let error = locate_with_timeout(
        &cli,
        dir.path(),
        "boom",
        false,
        10,
        Duration::from_millis(300),
    )
    .expect_err("must time out");
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
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("no-such-raios");
    let error = locate_with(&missing, dir.path(), "boom", false, 10).expect_err("must fail");
    assert_eq!(error.code, ErrorCode::Io);
    assert!(error.message.contains("başlatılamadı"));
}
