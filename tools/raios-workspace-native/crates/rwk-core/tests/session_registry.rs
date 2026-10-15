//! `SessionRegistry` against a fake `TerminalHost`.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rwk_core::error::{AppError, ErrorCode, Result};
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{
    ResumeKey, SessionId, SessionKind, SessionLaunch, SessionRegistry, TerminalHost, TerminalSize,
    MAX_SESSIONS,
};
use tempfile::TempDir;

type Spawn = (SessionKind, SessionLaunch, TerminalSize, PathBuf);

/// A fresh run with no first prompt.
fn fresh() -> SessionLaunch {
    SessionLaunch::default()
}

/// A fresh run with one first prompt.
fn fresh_with(prompt: &str) -> SessionLaunch {
    SessionLaunch::Fresh {
        prompt: Some(prompt.to_owned()),
    }
}

#[derive(Default)]
struct HostState {
    next: u32,
    running: HashSet<String>,
    spawned: Vec<Spawn>,
    stopped: Vec<String>,
    fail_spawn: bool,
    fail_stop_first: bool,
}

#[derive(Default)]
struct FakeHost {
    state: Mutex<HostState>,
}

impl FakeHost {
    fn finish(&self, id: &SessionId) {
        self.state.lock().unwrap().running.remove(id.as_str());
    }

    fn stopped(&self) -> Vec<String> {
        self.state.lock().unwrap().stopped.clone()
    }

    fn spawned(&self) -> Vec<Spawn> {
        self.state.lock().unwrap().spawned.clone()
    }
}

/// Newtype so the test can keep a shared handle to the host the registry owns.
struct Handle(Arc<FakeHost>);

impl TerminalHost for Handle {
    fn spawn(
        &self,
        project: &ProjectRoot,
        kind: SessionKind,
        launch: &SessionLaunch,
        size: TerminalSize,
    ) -> Result<SessionId> {
        let mut state = self.0.state.lock().unwrap();
        if state.fail_spawn {
            return Err(AppError::new(ErrorCode::Io, "no pty available"));
        }
        state.next += 1;
        let id = format!("s{}", state.next);
        state.running.insert(id.clone());
        state
            .spawned
            .push((kind, launch.clone(), size, project.info().root.clone()));
        Ok(SessionId::new(id))
    }

    fn write(&self, _id: &SessionId, _data: &[u8]) -> Result<()> {
        Ok(())
    }

    fn resize(&self, _id: &SessionId, _size: TerminalSize) -> Result<()> {
        Ok(())
    }

    fn stop(&self, id: &SessionId) -> Result<()> {
        let mut state = self.0.state.lock().unwrap();
        state.stopped.push(id.as_str().to_owned());
        if state.fail_stop_first && state.stopped.len() == 1 {
            return Err(AppError::new(ErrorCode::Io, "kill failed"));
        }
        state.running.remove(id.as_str());
        Ok(())
    }

    fn is_running(&self, id: &SessionId) -> bool {
        self.0.state.lock().unwrap().running.contains(id.as_str())
    }
}

const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

fn fixture() -> (TempDir, ProjectRoot, Arc<FakeHost>, SessionRegistry) {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectRoot::open(directory.path().to_path_buf()).unwrap();
    let host = Arc::new(FakeHost::default());
    let registry = SessionRegistry::new(Box::new(Handle(host.clone())));
    (directory, project, host, registry)
}

#[test]
fn a_started_session_is_listed_with_its_project_and_kind() {
    let (_directory, project, host, registry) = fixture();

    let started = registry
        .start(&project, SessionKind::Claude, &fresh_with("hello"), SIZE)
        .unwrap();

    assert_eq!(started.kind, SessionKind::Claude);
    assert_eq!(started.project_id, project.info().id);
    assert!(started.running);
    assert_eq!(registry.list(), vec![started]);
    assert_eq!(
        host.spawned(),
        vec![(
            SessionKind::Claude,
            fresh_with("hello"),
            SIZE,
            project.info().root.clone()
        )]
    );
}

#[test]
fn a_resume_reaches_the_host_with_its_validated_key() {
    let (_directory, project, host, registry) = fixture();
    let key = ResumeKey::parse("claude:3294c005-aee0-4e95-860f-5800e045b624").expect("key");

    let started = registry
        .start(
            &project,
            SessionKind::Claude,
            &SessionLaunch::Resume(key.clone()),
            SIZE,
        )
        .unwrap();

    assert_eq!(started.kind, SessionKind::Claude);
    assert_eq!(
        host.spawned(),
        vec![(
            SessionKind::Claude,
            SessionLaunch::Resume(key),
            SIZE,
            project.info().root.clone()
        )]
    );
}

#[test]
fn at_most_four_sessions_run_at_once_and_a_finished_one_frees_a_slot() {
    let (_directory, project, host, registry) = fixture();
    let mut started = Vec::new();
    for _ in 0..MAX_SESSIONS {
        started.push(
            registry
                .start(&project, SessionKind::Shell, &fresh(), SIZE)
                .unwrap(),
        );
    }

    let refused = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    assert_eq!(host.spawned().len(), MAX_SESSIONS, "nothing was spawned");

    host.finish(&started[0].id);
    let replacement = registry
        .start(&project, SessionKind::Codex, &fresh(), SIZE)
        .unwrap();

    let listed = registry.list();
    assert_eq!(listed.len(), MAX_SESSIONS);
    assert_eq!(listed.last().unwrap(), &replacement);
    assert!(listed.iter().all(|session| session.running));
}

#[test]
fn the_host_reporting_an_exit_marks_the_session_as_not_running() {
    let (_directory, project, host, registry) = fixture();
    let session = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap();

    host.finish(&session.id);

    let listed = registry.list();
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].running);
}

#[test]
fn stopping_a_session_stops_it_on_the_host() {
    let (_directory, project, host, registry) = fixture();
    let session = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap();

    registry.stop(&session.id).unwrap();

    assert_eq!(host.stopped(), vec![session.id.as_str().to_owned()]);
    assert!(!registry.list()[0].running);
}

#[test]
fn stopping_a_session_this_registry_does_not_own_is_not_found() {
    let (_directory, _project, host, registry) = fixture();

    let error = registry.stop(&SessionId::new("not-mine")).unwrap_err();

    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(host.stopped().is_empty());
}

#[test]
fn a_failed_spawn_leaves_the_registry_unchanged() {
    let (_directory, project, host, registry) = fixture();
    host.state.lock().unwrap().fail_spawn = true;

    let error = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Io);
    assert!(registry.list().is_empty());
}

#[test]
fn shutdown_stops_every_session_and_refuses_new_ones() {
    let (_directory, project, host, registry) = fixture();
    let first = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap();
    let second = registry
        .start(&project, SessionKind::Claude, &fresh(), SIZE)
        .unwrap();

    registry.stop_all();
    registry.stop_all(); // idempotent

    let stopped = host.stopped();
    assert!(stopped.contains(&first.id.as_str().to_owned()));
    assert!(stopped.contains(&second.id.as_str().to_owned()));
    assert!(registry.list().iter().all(|session| !session.running));
    let error = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::WorkspaceClosed);
}

#[test]
fn shutdown_stops_the_remaining_sessions_even_when_one_stop_fails() {
    let (_directory, project, host, registry) = fixture();
    host.state.lock().unwrap().fail_stop_first = true;
    let first = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap();
    let second = registry
        .start(&project, SessionKind::Shell, &fresh(), SIZE)
        .unwrap();

    registry.stop_all();

    let stopped = host.stopped();
    assert!(stopped.contains(&first.id.as_str().to_owned()));
    assert!(stopped.contains(&second.id.as_str().to_owned()));
    assert!(registry.list().iter().all(|session| !session.running));
}
