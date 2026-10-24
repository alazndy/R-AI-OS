//! The per-agent matrix of the workspace: each agent kind launches, takes input, resizes,
//! resumes and exits through the same PTY host. A small script that imitates the `raios run` and
//! `raios resume` argument shapes stands in for the wrapper, so no model, account or network is
//! involved; what is checked is what the workspace does, not what an agent answers.

#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{
    ResumeKey, SessionId, SessionKind, SessionLaunch, SessionName, TerminalHost, TerminalSize,
};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};

const WAIT: Duration = Duration::from_secs(15);

/// Every agent the workspace can start, with what the wrapper and the daemon call it.
/// (kind, `raios run` argument, daemon identity, native agent name used by `raios resume`)
const MATRIX: [(SessionKind, &str, &str, Option<&str>); 4] = [
    (
        SessionKind::Claude,
        "claude",
        "claude_kaira",
        Some("claude"),
    ),
    (SessionKind::Codex, "codex", "codex_kaira", Some("codex")),
    (
        SessionKind::Opencode,
        "opencode",
        "opencode_kaira",
        Some("opencode"),
    ),
    (SessionKind::Agy, "agy", "antigravity_kaira", None),
];

/// Imitates the argument shapes of `raios run <agent> --project <root> [--name N] [prompt]` and
/// `raios resume <key>`, then reads one line, reports the terminal size and exits.
const FAKE_RAIOS: &str = r#"
sub=$1; shift
if [ "$sub" = run ]; then
  agent=$1; shift; project=; name=none; rest=()
  while [ $# -gt 0 ]; do
    case $1 in
      --project) project=$2; shift 2;;
      --name) name=$2; shift 2;;
      *) rest+=("$1"); shift;;
    esac
  done
  echo "FAKE-RUN agent=$agent project=$project name=$name prompt=${rest[*]:-none} cwd=$PWD term=$TERM"
elif [ "$sub" = resume ]; then
  echo "FAKE-RESUME key=$1 cwd=$PWD"
else
  echo "FAKE-UNKNOWN $sub"; exit 64
fi
read -r line
echo "GOT:$line"
echo "SIZE:$(stty size)"
exit 7
"#;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn fake_raios(&self) -> PathBuf {
        let path = self.dir.path().join("raios-fake");
        std::fs::write(&path, format!("#!/bin/bash\n{FAKE_RAIOS}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn project(&self) -> ProjectRoot {
        let root = self.dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        ProjectRoot::open(root).unwrap()
    }

    fn host(&self) -> PtyTerminalHost {
        PtyTerminalHost::new(
            ProgramPaths {
                shell: None,
                raios: Some(self.fake_raios()),
            },
            Arc::new(|_: &SessionId| {}),
        )
    }
}

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

/// Spawns, retrying while the kernel reports the freshly written script as busy.
fn spawn(
    host: &PtyTerminalHost,
    project: &ProjectRoot,
    kind: SessionKind,
    launch: &SessionLaunch,
) -> SessionId {
    for _ in 0..20 {
        match host.spawn(project, kind, launch, size(240, 24)) {
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

#[test]
fn the_matrix_names_match_what_the_wrapper_and_the_daemon_expect() {
    for (kind, agent, identity, native) in MATRIX {
        assert_eq!(kind.agent(), Some(agent), "{kind:?} run argument");
        assert_eq!(kind.identity(), Some(identity), "{kind:?} daemon identity");
        assert_eq!(kind.native_agent(), native, "{kind:?} native agent");
        // Identities are lowercase words joined by `_`, the shape the daemon stores.
        assert!(identity.ends_with("_kaira"), "{identity}");
        assert!(!identity.contains('-'), "{identity}");
    }
    assert_eq!(SessionKind::Shell.agent(), None);
    assert_eq!(SessionKind::Shell.identity(), None);
    assert_eq!(SessionKind::Shell.native_agent(), None);
}

#[test]
fn every_agent_starts_named_with_a_prompt_in_the_project_and_takes_input_and_a_resize() {
    for (kind, agent, _, _) in MATRIX {
        let fixture = Fixture::new();
        let host = fixture.host();
        let project = fixture.project();
        let launch = SessionLaunch::Fresh {
            prompt: Some("fix the build".to_owned()),
            name: Some(SessionName::parse("api box").unwrap()),
        };
        let id = spawn(&host, &project, kind, &launch);

        let root = project.info().root.display().to_string();
        wait_for_screen(
            &host,
            &id,
            &format!("FAKE-RUN agent={agent} project={root} name=api box prompt=fix the build"),
        );
        let text = screen(&host, &id);
        assert!(text.contains(&format!("cwd={root}")), "{agent}: {text}");
        assert!(text.contains("term=xterm-256color"), "{agent}: {text}");

        host.resize(&id, size(100, 30)).unwrap();
        host.write(&id, b"ping\n").unwrap();
        wait_for_screen(&host, &id, "GOT:ping");
        wait_for_screen(&host, &id, "SIZE:30 100");

        wait_for(&format!("{agent} to exit"), || !host.is_running(&id));
    }
}

#[test]
fn an_unnamed_agent_run_carries_no_name_argument() {
    for (kind, agent, _, _) in MATRIX {
        let fixture = Fixture::new();
        let host = fixture.host();
        let id = spawn(&host, &fixture.project(), kind, &SessionLaunch::default());
        wait_for_screen(&host, &id, &format!("FAKE-RUN agent={agent} "));
        let text = screen(&host, &id);
        assert!(text.contains("name=none prompt=none"), "{agent}: {text}");
        host.write(&id, b"done\n").unwrap();
        wait_for("the run to exit", || !host.is_running(&id));
    }
}

#[test]
fn the_agents_with_a_native_continuation_resume_by_key_and_the_others_cannot() {
    for (kind, agent, _, native) in MATRIX {
        let key_text = format!("{agent}:3294c005-aee0-4e95-860f-5800e045b624");
        let Some(native) = native else {
            assert!(
                ResumeKey::parse(&key_text).is_none(),
                "{agent} has no resume"
            );
            continue;
        };
        assert_eq!(native, agent);
        let key = ResumeKey::parse(&key_text).expect("a valid key");
        assert_eq!(key.agent(), kind);

        let fixture = Fixture::new();
        let host = fixture.host();
        let project = fixture.project();
        let id = spawn(&host, &project, kind, &SessionLaunch::Resume(key));
        wait_for_screen(
            &host,
            &id,
            &format!(
                "FAKE-RESUME key={key_text} cwd={}",
                project.info().root.display()
            ),
        );
        host.write(&id, b"done\n").unwrap();
        wait_for("the run to exit", || !host.is_running(&id));
    }
}

#[test]
fn a_resume_key_of_another_agent_never_starts_a_session() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let key = ResumeKey::parse("codex:3294c005-aee0-4e95-860f-5800e045b624").unwrap();
    let error = host
        .spawn(
            &fixture.project(),
            SessionKind::Claude,
            &SessionLaunch::Resume(key),
            size(80, 24),
        )
        .unwrap_err();
    assert_eq!(error.code, rwk_core::error::ErrorCode::InvalidInput);
}
