//! PTY-backed sessions: a child process on a pseudo-terminal, feeding a [`TerminalEngine`].
//!
//! Linux only, like the rest of the workspace. Process ownership is carried over from
//! Workspace 0.1: the child stays unreaped while it is signalled, so its PID cannot be reused,
//! and descendants are signalled through pidfds rather than raw PIDs.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, MasterPty, PtySize};

use super::command::{build_command, ProgramPaths};
use super::engine::{EngineOptions, TerminalEngine};
use crate::error::{AppError, ErrorCode, Result};
use crate::project::ProjectRoot;
use crate::sessions::{SessionId, SessionKind, TerminalHost, TerminalSize};

/// How long a stopped agent wrapper gets to close its own run before it is killed too.
const AGENT_GRACE: Duration = Duration::from_secs(5);
/// How long output that is still in flight is drained after the child has exited.
const EXIT_DRAIN: Duration = Duration::from_millis(750);
/// Marker for an exit code that is not known (the child was lost, not reaped).
const UNKNOWN_EXIT: u32 = u32::MAX;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn pty_error(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Io, format!("terminal error: {error}"))
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

struct Session {
    kind: SessionKind,
    pid: i32,
    running: AtomicBool,
    exit_code: AtomicU32,
    master: Mutex<Box<dyn MasterPty + Send>>,
    // Signalling and reaping share this lock, so no stale raw-PID killer survives an exit.
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
    engine: Arc<TerminalEngine>,
    cancel_reader: Arc<AtomicBool>,
}

/// Starts and owns PTY sessions. Implements [`TerminalHost`] for the session registry and also
/// hands the UI the engine of each session.
pub struct PtyTerminalHost {
    programs: ProgramPaths,
    options: EngineOptions,
    wake: Arc<dyn Fn(&SessionId) + Send + Sync>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl PtyTerminalHost {
    /// Creates a host. `wake` is called, from a background thread, when a session has new
    /// output or has exited; it must be cheap and must not call back into the host.
    pub fn new(programs: ProgramPaths, wake: Arc<dyn Fn(&SessionId) + Send + Sync>) -> Self {
        Self::with_options(programs, EngineOptions::default(), wake)
    }

    /// Like [`new`](Self::new) with engine parameters other than the defaults. The size in
    /// `options` is ignored: each session starts at the size given to `spawn`.
    pub fn with_options(
        programs: ProgramPaths,
        options: EngineOptions,
        wake: Arc<dyn Fn(&SessionId) + Send + Sync>,
    ) -> Self {
        Self {
            programs,
            options,
            wake,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// The engine of a session, for the UI to snapshot and to send input to.
    pub fn engine(&self, id: &SessionId) -> Option<Arc<TerminalEngine>> {
        self.find(id).ok().map(|session| session.engine.clone())
    }

    /// The exit code of a finished session; `None` while it runs or when the code is unknown.
    pub fn exit_code(&self, id: &SessionId) -> Option<u32> {
        let session = self.find(id).ok()?;
        if session.running.load(Ordering::Acquire) {
            return None;
        }
        match session.exit_code.load(Ordering::Acquire) {
            UNKNOWN_EXIT => None,
            code => Some(code),
        }
    }

    /// Forgets a finished session and frees its terminal state. Running sessions are kept.
    pub fn remove(&self, id: &SessionId) -> bool {
        let mut sessions = lock(&self.sessions);
        let finished = sessions
            .get(id.as_str())
            .is_some_and(|session| !session.running.load(Ordering::Acquire));
        if finished {
            sessions.remove(id.as_str());
        }
        finished
    }

    fn find(&self, id: &SessionId) -> Result<Arc<Session>> {
        lock(&self.sessions)
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No such terminal session."))
    }
}

impl TerminalHost for PtyTerminalHost {
    fn spawn(
        &self,
        project: &ProjectRoot,
        kind: SessionKind,
        prompt: Option<&str>,
        size: TerminalSize,
    ) -> Result<SessionId> {
        let command = build_command(&self.programs, &project.info().root, kind, prompt)?;
        let pair = native_pty_system()
            .openpty(pty_size(size))
            .map_err(pty_error)?;
        let reader = pair.master.try_clone_reader().map_err(pty_error)?;
        let writer = pair.master.take_writer().map_err(pty_error)?;
        let cancel_reader = Arc::new(AtomicBool::new(false));
        let reader = polling_reader(&*pair.master, reader, cancel_reader.clone())?;

        let id = SessionId::new(uuid::Uuid::new_v4().to_string());
        let wake = self.wake.clone();
        let wake_id = id.clone();
        let engine = Arc::new(TerminalEngine::start(
            reader,
            writer,
            EngineOptions {
                size,
                ..self.options.clone()
            },
            Arc::new(move || wake(&wake_id)),
        )?);
        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                cancel_reader.store(true, Ordering::Release);
                return Err(pty_error(error));
            }
        };
        // The parent must not keep the slave open, or the master never sees end of output.
        drop(pair.slave);

        let session = Arc::new(Session {
            kind,
            pid: child.process_id().map_or(0, |pid| pid as i32),
            running: AtomicBool::new(true),
            exit_code: AtomicU32::new(UNKNOWN_EXIT),
            master: Mutex::new(pair.master),
            child: Mutex::new(Some(child)),
            engine,
            cancel_reader,
        });
        lock(&self.sessions).insert(id.as_str().to_owned(), session.clone());
        let wake = self.wake.clone();
        let watched_id = id.clone();
        thread::Builder::new()
            .name("rwk-term-watch".into())
            .spawn(move || watch(&session, &*wake, &watched_id))
            .map_err(|error| AppError::new(ErrorCode::Internal, error.to_string()))?;
        Ok(id)
    }

    fn write(&self, id: &SessionId, data: &[u8]) -> Result<()> {
        let session = self.find(id)?;
        if !session.running.load(Ordering::Acquire) {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "This terminal session has ended.",
            ));
        }
        session.engine.send_input(data)
    }

    fn resize(&self, id: &SessionId, size: TerminalSize) -> Result<()> {
        let session = self.find(id)?;
        session.engine.resize(size)?;
        let resized = lock(&session.master).resize(pty_size(size));
        resized.map_err(pty_error)
    }

    fn stop(&self, id: &SessionId) -> Result<()> {
        let session = self.find(id)?;
        stop_session(&session);
        Ok(())
    }

    fn is_running(&self, id: &SessionId) -> bool {
        self.find(id)
            .is_ok_and(|session| session.running.load(Ordering::Acquire))
    }
}

/// Waits for the child, then cleans up what it left behind and tells the UI.
fn watch(session: &Session, wake: &(dyn Fn(&SessionId) + Send + Sync), id: &SessionId) {
    let code = loop {
        let mut owned = lock(&session.child);
        let Some(child) = owned.as_mut() else {
            break UNKNOWN_EXIT;
        };
        // The child is observed without being reaped, so descendants that still hold the PTY
        // open are cleaned up while its PID and session identity are still valid.
        if session.pid > 1 && exited_unreaped(session.pid) {
            kill_session_descendants(session.pid);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                *owned = None;
                break status.exit_code();
            }
            Err(_) => {
                *owned = None;
                break UNKNOWN_EXIT;
            }
            Ok(None) => {}
        }
        drop(owned);
        thread::sleep(Duration::from_millis(25));
    };
    session.exit_code.store(code, Ordering::Release);
    session.running.store(false, Ordering::Release);
    // Output still in flight is drained for a bounded time; a detached descendant must never
    // hold the exit hostage.
    let deadline = Instant::now() + EXIT_DRAIN;
    while !session.engine.is_output_closed() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    session.cancel_reader.store(true, Ordering::Release);
    wake(id);
}

fn stop_session(session: &Session) {
    if !session.running.load(Ordering::Acquire) {
        return;
    }
    if session.kind.agent().is_some() && session.pid > 1 {
        // End the agent's own children first. The RAIOS wrapper then sees its child exit and
        // closes its run, instead of being killed at the same time and leaving a stale row.
        {
            let child = lock(&session.child);
            if child.is_none() {
                return;
            }
            kill_session_descendants(session.pid);
        }
        let deadline = Instant::now() + AGENT_GRACE;
        while session.running.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if !session.running.load(Ordering::Acquire) {
            return;
        }
    }
    let child = lock(&session.child);
    if child.is_none() {
        return;
    }
    kill_session_descendants(session.pid);
    if session.pid > 1 {
        // The child is still unreaped under this lock, so the PID cannot belong to anyone else.
        // SAFETY: kill takes plain integers.
        unsafe { libc::kill(session.pid, libc::SIGKILL) };
    }
}

/// True when the child has exited but has not been reaped yet.
fn exited_unreaped(pid: i32) -> bool {
    // SAFETY: a zeroed siginfo_t is a valid out-parameter for waitid.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: waitid receives an initialized siginfo_t; WNOWAIT leaves the child reapable.
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // SAFETY: reading si_pid of a siginfo_t that waitid filled in.
    status == 0 && unsafe { info.si_pid() } != 0
}

/// Sends SIGKILL to every process of the session led by `session_pid`, except the leader.
fn kill_session_descendants(session_pid: i32) {
    if session_pid <= 1 {
        return;
    }
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if pid <= 1 || pid == session_pid {
            continue;
        }
        // A pidfd pins the process before ownership is checked, so a reused number cannot be
        // signalled by mistake.
        // SAFETY: pidfd_open takes plain integers and returns a descriptor or -1.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if raw < 0 {
            continue;
        }
        // SAFETY: pidfd_open returned a fresh descriptor that nothing else owns.
        let pidfd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        // SAFETY: getsid takes a plain integer.
        if unsafe { libc::getsid(pid) } == session_pid {
            // SAFETY: valid pidfd, no siginfo, no flags.
            unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    pidfd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                );
            }
        }
    }
}

/// A PTY reader that gives up when asked, so a detached descendant holding the slave open
/// cannot keep the reader thread alive forever.
struct PollingReader {
    inner: Box<dyn Read + Send>,
    fd: OwnedFd,
    cancelled: Arc<AtomicBool>,
}

impl Read for PollingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        while !self.cancelled.load(Ordering::Acquire) {
            let mut descriptor = libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll receives one initialized descriptor owned by this reader.
            let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
            if ready > 0 {
                return self.inner.read(buffer);
            }
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
        Ok(0)
    }
}

fn polling_reader(
    master: &dyn MasterPty,
    inner: Box<dyn Read + Send>,
    cancelled: Arc<AtomicBool>,
) -> Result<Box<dyn Read + Send>> {
    let raw = master
        .as_raw_fd()
        .ok_or_else(|| pty_error("missing PTY descriptor"))?;
    // SAFETY: the master owns `raw` for the duration of this call.
    let fd = unsafe { BorrowedFd::borrow_raw(raw) }
        .try_clone_to_owned()
        .map_err(pty_error)?;
    Ok(Box::new(PollingReader {
        inner,
        fd,
        cancelled,
    }))
}
