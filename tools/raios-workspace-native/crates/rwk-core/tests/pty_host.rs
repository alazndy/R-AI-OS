//! The PTY host with real child processes: small shell scripts stand in for the shell and for
//! the `raios` wrapper.

#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rwk_core::error::ErrorCode;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{
    ResumeKey, SessionId, SessionKind, SessionLaunch, TerminalHost, TerminalSize,
};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};

const WAIT: Duration = Duration::from_secs(15);

fn size(cols: u16, rows: u16) -> TerminalSize {
    TerminalSize { cols, rows }
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A project directory plus an executable script written into it.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn project(&self) -> ProjectRoot {
        ProjectRoot::open(self.dir.path().to_path_buf()).unwrap()
    }
}

fn host(programs: ProgramPaths) -> PtyTerminalHost {
    PtyTerminalHost::new(programs, Arc::new(|_: &SessionId| {}))
}

fn shell_host(shell: PathBuf) -> PtyTerminalHost {
    host(ProgramPaths {
        shell: Some(shell),
        raios: None,
    })
}

/// Spawns, retrying when the kernel reports the freshly written script as busy: a thread of a
/// parallel test may hold a copy of its descriptor for a moment during `fork`.
fn spawn(
    host: &PtyTerminalHost,
    project: &ProjectRoot,
    kind: SessionKind,
    prompt: Option<&str>,
    terminal: TerminalSize,
) -> SessionId {
    let launch = SessionLaunch::Fresh {
        prompt: prompt.map(str::to_owned),
        name: None,
    };
    spawn_launch(host, project, kind, &launch, terminal)
}

/// Like [`spawn`], for a launch that is not a fresh run (a resume, say).
fn spawn_launch(
    host: &PtyTerminalHost,
    project: &ProjectRoot,
    kind: SessionKind,
    launch: &SessionLaunch,
    terminal: TerminalSize,
) -> SessionId {
    for _ in 0..20 {
        match host.spawn(project, kind, launch, terminal) {
            Ok(id) => return id,
            Err(error) if error.message.contains("Text file busy") => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("spawn failed: {error:?}"),
        }
    }
    panic!("spawn kept failing with a busy script");
}

fn screen(host: &PtyTerminalHost, id: &SessionId) -> String {
    host.engine(id).unwrap().snapshot().screen_text()
}

fn wait_for_screen(host: &PtyTerminalHost, id: &SessionId, needle: &str) {
    wait_for(&format!("{needle:?} on the screen"), || {
        screen(host, id).contains(needle)
    });
}

fn process_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
        && std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|stat| !stat.contains(") Z "))
}

fn read_pid(path: &Path) -> i32 {
    wait_for("the pid file", || {
        path.exists() && std::fs::metadata(path).is_ok_and(|m| m.len() > 1)
    });
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[test]
fn program_output_reaches_the_snapshot_and_the_exit_code_is_recorded() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script("shell", "echo hello from the pty; exit 3"));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );

    wait_for_screen(&host, &id, "hello from the pty");
    wait_for("the exit", || !host.is_running(&id));
    assert_eq!(host.exit_code(&id), Some(3));
    assert!(host.engine(&id).unwrap().wait_output_closed(WAIT));
}

#[test]
fn the_session_starts_in_the_project_with_the_terminal_environment() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script(
        "shell",
        "echo \"dir=$(pwd) TERM=$TERM COLORTERM=$COLORTERM RUN=${RAIOS_WRAPPER_RUN_ID:-unset} ID=${RAIOS_AGENT_IDENTITY:-unset}\"",
    ));
    // The parent's agent identity must not leak into the session.
    std::env::set_var("RAIOS_WRAPPER_RUN_ID", "leaked-run");
    std::env::set_var("RAIOS_AGENT_IDENTITY", "leaked-agent");
    let project = fixture.project();
    let id = spawn(&host, &project, SessionKind::Shell, None, size(200, 24));

    let expected = format!(
        "dir={} TERM=xterm-256color COLORTERM=truecolor RUN=unset ID=unset",
        project.info().root.display()
    );
    wait_for_screen(&host, &id, &expected);
}

#[test]
fn typed_input_reaches_the_program() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script("shell", "read -r line; echo \"got:$line\""));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );

    host.write(&id, b"hello\n").unwrap();

    wait_for_screen(&host, &id, "got:hello");
}

#[test]
fn resizing_changes_the_size_the_program_sees() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script("shell", "stty size; read -r x; stty size"));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(100, 30),
    );
    wait_for_screen(&host, &id, "30 100");

    host.resize(&id, size(120, 40)).unwrap();
    host.write(&id, b"\n").unwrap();

    wait_for_screen(&host, &id, "40 120");
    let snapshot = host.engine(&id).unwrap().snapshot();
    assert_eq!((snapshot.cols, snapshot.rows), (120, 40));
    assert_eq!(
        host.resize(&id, size(1, 1)).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn a_cursor_position_query_makes_the_round_trip_through_the_real_pty() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script(
        "shell",
        "stty raw -echo\nprintf 'abc\\033[6n'\nIFS= read -r -d R reply\nstty sane\nprintf '\\nGOT[%s]\\n' \"$(printf %s \"$reply\" | od -An -c | tr -d ' ')\"",
    ));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );

    wait_for_screen(&host, &id, "GOT[033[1;4]");
}

#[test]
fn stopping_a_shell_kills_it_and_everything_it_started() {
    let fixture = Fixture::new();
    let pid_file = fixture.file("sleeper.pid");
    let host = shell_host(fixture.script(
        "shell",
        // Ignoring SIGHUP matters: otherwise the kernel's hang-up would end the sleeper by
        // itself and the test would not prove that the host kills it.
        &format!(
            "trap '' HUP\nsleep 600 &\necho $! > {}\nwait",
            pid_file.display()
        ),
    ));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );
    let sleeper = read_pid(&pid_file);
    assert!(process_alive(sleeper));

    host.stop(&id).unwrap();

    wait_for("the session to end", || !host.is_running(&id));
    wait_for("the background process to die", || !process_alive(sleeper));
}

#[test]
fn stopping_an_agent_lets_the_wrapper_finish_before_it_is_killed() {
    let fixture = Fixture::new();
    let pid_file = fixture.file("child.pid");
    let done = fixture.file("wrapper.done");
    let wrapper = fixture.script(
        "raios",
        &format!(
            "trap '' HUP\necho \"args: $*\"\nsleep 600 &\necho $! > {}\nwait\necho finished > {}",
            pid_file.display(),
            done.display()
        ),
    );
    let host = host(ProgramPaths {
        shell: None,
        raios: Some(wrapper),
    });
    let project = fixture.project();
    let id = spawn(
        &host,
        &project,
        SessionKind::Claude,
        Some("look around"),
        size(160, 24),
    );
    let child = read_pid(&pid_file);
    wait_for_screen(
        &host,
        &id,
        &format!(
            "args: run claude --project {} look around",
            project.info().root.display()
        ),
    );

    host.stop(&id).unwrap();

    wait_for("the session to end", || !host.is_running(&id));
    assert!(!process_alive(child));
    assert!(
        done.exists(),
        "the wrapper must get to finish its own cleanup"
    );
    assert_eq!(host.exit_code(&id), Some(0));
}

#[test]
fn a_resume_pane_runs_raios_resume_with_the_exact_key() {
    let fixture = Fixture::new();
    let wrapper = fixture.script("raios", "echo \"args: $*\"\nsleep 600");
    let host = host(ProgramPaths {
        shell: None,
        raios: Some(wrapper),
    });
    let project = fixture.project();
    let key = ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("key");
    let id = spawn_launch(
        &host,
        &project,
        SessionKind::Claude,
        &SessionLaunch::Resume(key),
        size(140, 24),
    );

    wait_for_screen(
        &host,
        &id,
        "args: resume claude:3294c005-aee0-4e95-860f-5800e045b624",
    );

    host.stop(&id).unwrap();
    wait_for("the session to end", || !host.is_running(&id));
}

#[test]
fn a_detached_background_process_cannot_hold_the_exit_hostage() {
    let fixture = Fixture::new();
    let pid_file = fixture.file("detached.pid");
    // `setsid` puts the sleeper in its own session, so it keeps the PTY open after the shell exits.
    let host = shell_host(fixture.script(
        "shell",
        &format!(
            "setsid sleep 30 &\necho $! > {}\nexit 0",
            pid_file.display()
        ),
    ));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );
    let sleeper = read_pid(&pid_file);

    wait_for("the exit to be reported", || !host.is_running(&id));
    assert_eq!(host.exit_code(&id), Some(0));

    // Clean up the process the test itself created.
    // SAFETY: kill takes plain integers; the pid belongs to this test's own sleeper.
    unsafe { libc_kill(sleeper) };
}

unsafe fn libc_kill(pid: i32) {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    kill(pid, 9);
}

#[test]
fn a_bad_prompt_or_unknown_session_is_refused() {
    let fixture = Fixture::new();
    let host = host(ProgramPaths {
        shell: None,
        raios: Some(fixture.script("raios", "true")),
    });
    let project = fixture.project();
    for prompt in ["--dangerous", "bad\0prompt", &"x".repeat(601)] {
        let error = host
            .spawn(
                &project,
                SessionKind::Codex,
                &SessionLaunch::Fresh {
                    prompt: Some(prompt.to_owned()),
                    name: None,
                },
                size(80, 24),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{prompt:?}");
    }
    let missing = SessionId::new("no-such-session");
    assert_eq!(
        host.write(&missing, b"x").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        host.resize(&missing, size(80, 24)).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(host.stop(&missing).unwrap_err().code, ErrorCode::NotFound);
    assert!(!host.is_running(&missing));
    assert!(host.engine(&missing).is_none());
}

#[test]
fn writing_to_a_finished_session_and_removing_it() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script("shell", "exit 0"));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );
    wait_for("the exit", || !host.is_running(&id));

    assert_eq!(host.write(&id, b"x").unwrap_err().code, ErrorCode::NotFound);
    assert!(host.remove(&id));
    assert!(host.engine(&id).is_none());
}

#[test]
fn a_running_session_is_not_removed() {
    let fixture = Fixture::new();
    let host = shell_host(fixture.script("shell", "sleep 600"));
    let id = spawn(
        &host,
        &fixture.project(),
        SessionKind::Shell,
        None,
        size(80, 24),
    );

    assert!(!host.remove(&id));
    assert!(host.is_running(&id));
    host.stop(&id).unwrap();
    wait_for("the session to end", || !host.is_running(&id));
}
