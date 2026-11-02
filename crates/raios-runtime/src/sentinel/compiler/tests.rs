use super::*;
use std::fs;
use tempfile::TempDir;

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A dependency-free crate with a generated lock file, so `--frozen` works
/// offline.
fn make_project(main_rs: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    write(
        &dir.path().join("Cargo.toml"),
        "[package]\nname = \"sentinel_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&dir.path().join("src/main.rs"), main_rs);
    let status = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("cargo available");
    assert!(status.success(), "generate-lockfile failed");
    dir
}

fn runs(project: &Path) -> u32 {
    state().runs.get(project).copied().unwrap_or(0)
}

// ─── fingerprint ─────────────────────────────────────────────────────────────

#[test]
fn fingerprint_changes_when_a_source_changes() {
    let dir = make_project("fn main() {}\n");
    let before = source_fingerprint(dir.path()).unwrap();
    assert_eq!(before, source_fingerprint(dir.path()).unwrap());

    write(
        &dir.path().join("src/main.rs"),
        "fn main() { let _x = 1; }\n",
    );
    assert_ne!(before, source_fingerprint(dir.path()).unwrap());
}

#[test]
fn fingerprint_ignores_target_git_hidden_and_non_sources() {
    let dir = make_project("fn main() {}\n");
    let before = source_fingerprint(dir.path()).unwrap();

    write(&dir.path().join("target/debug/junk.rs"), "x");
    write(&dir.path().join("node_modules/pkg/index.rs"), "x");
    write(&dir.path().join(".git/hooks/pre-commit.rs"), "x");
    write(&dir.path().join(".hidden/a.rs"), "x");
    write(&dir.path().join("README.md"), "docs");
    write(&dir.path().join("notes.txt"), "notes");

    assert_eq!(before, source_fingerprint(dir.path()).unwrap());
}

#[test]
fn fingerprint_of_a_missing_root_is_none() {
    assert!(source_fingerprint(Path::new("/definitely/not/a/real/path")).is_none());
}

#[cfg(unix)]
#[test]
fn fingerprint_does_not_follow_symlink_cycles() {
    let dir = make_project("fn main() {}\n");
    std::os::unix::fs::symlink(dir.path(), dir.path().join("src/loop")).unwrap();
    // Would recurse forever (or overflow the stack) if links were followed.
    assert!(source_fingerprint(dir.path()).is_some());
}

// ─── result interpretation ───────────────────────────────────────────────────

#[test]
fn a_failure_without_compiler_errors_is_an_error_not_a_clean_compile() {
    let output = CheckOutput {
        stdout: String::new(),
        stderr: "warning: noise\nerror: the lock file needs to be updated\n".into(),
        success: false,
        timed_out: false,
    };
    let err = interpret(output).unwrap_err().to_string();
    assert!(err.contains("lock file needs to be updated"), "{err}");
}

#[test]
fn a_successful_check_with_no_errors_is_clean() {
    let output = CheckOutput {
        success: true,
        ..Default::default()
    };
    assert!(interpret(output).unwrap().is_empty());
}

#[test]
fn compiler_errors_are_reported_with_file_and_line() {
    let line = r#"{"reason":"compiler-message","message":{"level":"error","message":"boom","spans":[{"file_name":"src/main.rs","line_start":3}]}}"#;
    let output = CheckOutput {
        stdout: format!("{line}\n"),
        success: false,
        ..Default::default()
    };
    let errors = interpret(output).unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].file, "src/main.rs");
    assert_eq!(errors[0].line, Some(3));
    assert_eq!(errors[0].message, "boom");
}

// ─── real cargo ──────────────────────────────────────────────────────────────

#[test]
fn clean_project_checks_clean_and_is_then_memoized() {
    let project = make_project("fn main() {}\n");
    let target = TempDir::new().unwrap();

    assert!(!is_up_to_date(project.path()));
    let first = cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    assert!(first.success, "{}", first.stderr);
    assert_eq!(runs(project.path()), 1);
    assert!(is_up_to_date(project.path()));

    // Unchanged sources: answered from the cache, no second process.
    let second = cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    assert!(second.success);
    assert_eq!(runs(project.path()), 1);
}

#[test]
fn editing_a_source_invalidates_the_memoized_result() {
    let project = make_project("fn main() {}\n");
    let target = TempDir::new().unwrap();

    cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    write(
        &project.path().join("src/main.rs"),
        "fn main() { let _y = 2; }\n",
    );
    assert!(!is_up_to_date(project.path()));
    cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    assert_eq!(runs(project.path()), 2);
}

#[test]
fn a_type_error_is_reported() {
    let project = make_project("fn main() { let x: u32 = \"no\"; }\n");
    let target = TempDir::new().unwrap();
    let output = cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    let errors = interpret(output).unwrap();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.file.ends_with("main.rs")));
}

#[test]
fn a_missing_lock_file_is_never_reported_as_compiled_and_is_not_created() {
    let project = make_project("fn main() {}\n");
    fs::remove_file(project.path().join("Cargo.lock")).unwrap();
    let target = TempDir::new().unwrap();

    let output = cargo_check_output_in(project.path(), target.path(), CHECK_TIMEOUT).unwrap();
    assert!(!output.success);
    assert!(
        interpret(output).is_err(),
        "must not look like a clean compile"
    );
    assert!(
        !project.path().join("Cargo.lock").exists(),
        "the user's project must not be modified"
    );
}

#[test]
fn a_hung_build_is_killed_at_the_timeout() {
    let project = make_project("fn main() {}\n");
    write(
        &project.path().join("build.rs"),
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(120)); }\n",
    );
    let target = TempDir::new().unwrap();

    let started = Instant::now();
    let output =
        cargo_check_output_in(project.path(), target.path(), Duration::from_secs(4)).unwrap();
    assert!(output.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "timeout did not stop the build: {:?}",
        started.elapsed()
    );
    assert!(failure_summary(&output).contains("timed out"));
    // A timed-out run is never cached as an answer.
    assert!(!is_up_to_date(project.path()));
}

#[test]
fn a_burst_of_concurrent_requests_costs_a_single_check() {
    // Models a formatter touching many files: every FileChanged event asks for
    // a check at once. They queue on the lock, the first one runs cargo, and
    // the rest are answered from its result.
    let project = make_project("fn main() {}\n");
    let target = TempDir::new().unwrap();
    let path = project.path().to_path_buf();
    let target_path = target.path().to_path_buf();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (path, target_path) = (path.clone(), target_path.clone());
            std::thread::spawn(move || {
                cargo_check_output_in(&path, &target_path, CHECK_TIMEOUT)
                    .unwrap()
                    .success
            })
        })
        .collect();
    for handle in handles {
        assert!(handle.join().unwrap());
    }
    assert_eq!(runs(&path), 1, "8 requests must share one cargo process");
}
