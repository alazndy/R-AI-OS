//! The one place the daemon runs `cargo check` on a user's project.
//!
//! Several background paths want compiler feedback (the Sentinel poll, the
//! per-file validation worker, the embedded TUI worker). They all share this
//! runner so the safety properties below hold everywhere:
//!
//! * **Read-only for the project**: `--frozen` means cargo never rewrites the
//!   user's `Cargo.lock` and never touches the network. A project whose lock
//!   file is missing or stale simply cannot be checked, and says so.
//! * **Own target directory** (`~/.cache/raios/sentinel-target`): the check
//!   never contends for the user's `target/` build lock while they run their
//!   own `cargo build`/`cargo test`. It is disposable cache — delete it freely.
//! * **Bounded cost**: one `cargo check` at a time process-wide, two jobs,
//!   lowered priority, and a hard timeout that kills the whole process group.
//! * **Memoized by source fingerprint**: if nothing under the project that can
//!   affect compilation changed since the last completed check, the cached
//!   result is returned and no process is started. A burst of `FileChanged`
//!   events therefore costs one check, not one per file.
//! * **Honest results**: a check that failed for a reason other than a
//!   compiler error (stale lock file, missing dependency, timeout) is an
//!   `Err`, never reported as "compiled".

use crate::daemon::state::ValidationError;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// Hard limit for a single `cargo check`.
const CHECK_TIMEOUT: Duration = Duration::from_secs(300);
/// Results with more output than this are not memoized (warning-heavy crates).
const MAX_CACHED_STDOUT: usize = 2 * 1024 * 1024;
/// Upper bound on files examined when fingerprinting one project.
const MAX_FINGERPRINT_FILES: u64 = 50_000;

// ─── Source fingerprint ──────────────────────────────────────────────────────

/// Cheap summary of everything under a project that can change a compile
/// result: newest modification time, file count and total size of the Rust
/// sources and Cargo manifests/lock file. `target/`, `.git`, `node_modules`
/// and hidden directories are ignored, and symlinks are never followed (so a
/// link cycle cannot loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFingerprint {
    pub newest: SystemTime,
    pub files: u64,
    pub bytes: u64,
}

/// Fingerprint of `root`, or `None` if it cannot be computed (unreadable
/// root, or more than [`MAX_FINGERPRINT_FILES`] relevant files). `None`
/// means "assume changed": the project is checked every time and never cached.
pub fn source_fingerprint(root: &Path) -> Option<SourceFingerprint> {
    let mut newest = SystemTime::UNIX_EPOCH;
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) if dir == root => return None,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_dir() {
                let skip =
                    matches!(name.as_ref(), "target" | "node_modules") || name.starts_with('.');
                if !skip {
                    stack.push(entry.path());
                }
            } else if is_relevant_source(&name) {
                let Ok(meta) = entry.metadata() else { continue };
                files += 1;
                if files > MAX_FINGERPRINT_FILES {
                    return None;
                }
                bytes += meta.len();
                if let Ok(modified) = meta.modified() {
                    newest = newest.max(modified);
                }
            }
        }
    }
    Some(SourceFingerprint {
        newest,
        files,
        bytes,
    })
}

fn is_relevant_source(file_name: &str) -> bool {
    file_name == "Cargo.lock" || file_name.ends_with(".rs") || file_name.ends_with(".toml")
}

// ─── Raw runner ──────────────────────────────────────────────────────────────

/// What a `cargo check` run produced.
#[derive(Debug, Clone, Default)]
pub struct CheckOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub timed_out: bool,
}

struct Cached {
    fingerprint: SourceFingerprint,
    output: CheckOutput,
}

#[derive(Default)]
struct State {
    cache: HashMap<PathBuf, Cached>,
    /// How many times a process was actually started per project (tests).
    runs: HashMap<PathBuf, u32>,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Serializes `cargo check` process-wide: never two at once, whoever asks.
static CHECK_LOCK: Mutex<()> = Mutex::new(());

/// Where check artifacts live. Disposable cache, kept apart from the user's
/// own `target/` directories.
pub fn default_target_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("raios")
        .join("sentinel-target")
}

/// True if a completed check already covers the project's current sources,
/// i.e. running [`run_cargo_check`] now would be answered from the cache.
pub fn is_up_to_date(project: &Path) -> bool {
    let Some(current) = source_fingerprint(project) else {
        return false;
    };
    state()
        .cache
        .get(project)
        .is_some_and(|cached| cached.fingerprint == current)
}

/// Runs (or answers from cache) `cargo check` for `project` using the default
/// target directory and timeout.
pub fn cargo_check_output(project: &Path) -> Result<CheckOutput> {
    cargo_check_output_in(project, &default_target_dir(), CHECK_TIMEOUT)
}

fn cargo_check_output_in(
    project: &Path,
    target_dir: &Path,
    timeout: Duration,
) -> Result<CheckOutput> {
    let _serial = CHECK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Fingerprint *after* acquiring the lock — a burst of callers queues up
    // here, and all but the first find a fresh cache entry — and *before*
    // spawning, so an edit made while cargo runs invalidates the entry.
    let fingerprint = source_fingerprint(project);
    if let Some(current) = &fingerprint {
        if let Some(cached) = state().cache.get(project) {
            if &cached.fingerprint == current {
                return Ok(cached.output.clone());
            }
        }
    }

    let output = spawn_and_wait(project, target_dir, timeout)?;
    let mut st = state();
    *st.runs.entry(project.to_path_buf()).or_insert(0) += 1;
    match fingerprint {
        Some(fingerprint) if !output.timed_out && output.stdout.len() <= MAX_CACHED_STDOUT => {
            st.cache.insert(
                project.to_path_buf(),
                Cached {
                    fingerprint,
                    output: output.clone(),
                },
            );
        }
        _ => {
            st.cache.remove(project);
        }
    }
    Ok(output)
}

fn spawn_and_wait(project: &Path, target_dir: &Path, timeout: Duration) -> Result<CheckOutput> {
    let mut cmd = Command::new("cargo");
    cmd.args(["check", "--frozen", "--jobs", "2", "--message-format=json"])
        .current_dir(project)
        .env("CARGO_TARGET_DIR", target_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group so a timeout can kill cargo *and* the rustc/build
        // scripts it spawned; and lower priority so a background check does
        // not compete with the user's foreground work.
        cmd.process_group(0);
        // SAFETY: only async-signal-safe `nice(2)` runs between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::nice(10);
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("could not start cargo: {e}"))?;

    // Drain both pipes on their own threads, or a chatty cargo fills the pipe
    // buffer while we poll for exit and the two deadlock.
    let mut stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("no stdout pipe"))?;
    let mut stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("no stderr pipe"))?;
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let (success, timed_out) = loop {
        match child.try_wait()? {
            Some(status) => break (status.success(), false),
            None if Instant::now() >= deadline => {
                kill_tree(&mut child);
                let _ = child.wait();
                break (false, true);
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };

    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    Ok(CheckOutput {
        stdout,
        stderr,
        success,
        timed_out,
    })
}

#[cfg(unix)]
fn kill_tree(child: &mut std::process::Child) {
    // SAFETY: plain `kill(2)` on the process group we created above.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn kill_tree(child: &mut std::process::Child) {
    let _ = child.kill();
}

// ─── Result interpretation ───────────────────────────────────────────────────

/// One-line reason for a check that failed without a compiler error.
pub fn failure_summary(output: &CheckOutput) -> String {
    if output.timed_out {
        return format!("cargo check timed out after {}s", CHECK_TIMEOUT.as_secs());
    }
    let lines = || {
        output
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
    };
    let line = lines()
        .find(|l| l.starts_with("error"))
        .or_else(|| lines().next_back())
        .unwrap_or("cargo check failed");
    line.chars().take(300).collect()
}

fn parse_errors(stdout: &str) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["reason"] != "compiler-message" {
            continue;
        }
        let message = &v["message"];
        if message["level"].as_str() != Some("error") {
            continue;
        }
        let text = message["message"].as_str().unwrap_or("").to_string();
        let Some(spans) = message["spans"].as_array() else {
            continue;
        };
        for span in spans {
            errors.push(ValidationError {
                file: span["file_name"].as_str().unwrap_or("").to_string(),
                message: text.clone(),
                line: span["line_start"].as_u64().map(|n| n as usize),
                source: "cargo check".to_string(),
            });
        }
    }
    errors
}

/// Compiler errors for `project_path`.
///
/// `Err` means the project could not be checked at all (cargo missing, stale
/// lock file, dependency not available offline, timeout) — callers must not
/// treat that as a clean compile.
pub fn run_cargo_check(project_path: &Path) -> Result<Vec<ValidationError>> {
    interpret(cargo_check_output(project_path)?)
}

fn interpret(output: CheckOutput) -> Result<Vec<ValidationError>> {
    let errors = parse_errors(&output.stdout);
    if errors.is_empty() && !output.success {
        return Err(anyhow!(failure_summary(&output)));
    }
    Ok(errors)
}

#[cfg(test)]
mod tests;
