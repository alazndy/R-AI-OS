//! Makes the wrapper's per-run cleanup independent of how the run ends.
//!
//! A wrapper run leaves two things behind that must not outlive it: the session block in
//! `~/.claude/CLAUDE.md` and the wrapper-note socket thread. `RunCleanup` owns both and releases
//! them from `Drop`, so an early `return` or a panic cleans up exactly like a normal exit.
//! `SignalGuard` covers the case `Drop` cannot: SIGTERM/SIGHUP kill a Rust process without
//! unwinding, so the wrapper turns them into a flag the wait loop polls and then exits normally.

use super::*;

/// Set by the SIGTERM/SIGHUP handler; polled by `wait_for_agent`.
static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(super) fn termination_requested() -> bool {
    TERMINATION_REQUESTED.load(Ordering::SeqCst)
}

/// While alive, SIGTERM and SIGHUP set `TERMINATION_REQUESTED` instead of killing the wrapper.
/// The previous handlers are restored on drop.
pub(super) struct SignalGuard {
    #[cfg(unix)]
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

#[cfg(unix)]
extern "C" fn on_termination_signal(_signal: libc::c_int) {
    // Only an atomic store: the one thing that is async-signal-safe here.
    TERMINATION_REQUESTED.store(true, Ordering::SeqCst);
}

impl SignalGuard {
    pub(super) fn install() -> Self {
        TERMINATION_REQUESTED.store(false, Ordering::SeqCst);
        #[cfg(unix)]
        {
            let mut previous = Vec::new();
            for signal in [libc::SIGTERM, libc::SIGHUP] {
                // SAFETY: `action` is fully initialised before the call, and the handler only
                // performs an atomic store.
                unsafe {
                    let mut action: libc::sigaction = std::mem::zeroed();
                    action.sa_sigaction = on_termination_signal as *const () as usize;
                    libc::sigemptyset(&mut action.sa_mask);
                    let mut old: libc::sigaction = std::mem::zeroed();
                    if libc::sigaction(signal, &action, &mut old) == 0 {
                        previous.push((signal, old));
                    }
                }
            }
            SignalGuard { previous }
        }
        #[cfg(not(unix))]
        SignalGuard {}
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        for (signal, old) in &self.previous {
            // SAFETY: restores exactly the action the kernel returned for this signal.
            unsafe {
                libc::sigaction(*signal, old, std::ptr::null_mut());
            }
        }
    }
}

/// Owns what a run must undo. `release` is idempotent, so the happy path can call it at the
/// exact point the run wants the cleanup to happen and `Drop` stays a backstop for every other
/// way out.
pub(super) struct RunCleanup {
    claude_md_injected: bool,
    #[cfg(unix)]
    note_ipc: Option<WrapperNoteIpc>,
}

impl RunCleanup {
    pub(super) fn new(
        claude_md_injected: bool,
        #[cfg(unix)] note_ipc: Option<WrapperNoteIpc>,
    ) -> Self {
        RunCleanup {
            claude_md_injected,
            #[cfg(unix)]
            note_ipc,
        }
    }

    pub(super) fn release(&mut self) {
        #[cfg(unix)]
        if let Some(ipc) = self.note_ipc.take() {
            ipc.stop.store(true, Ordering::Relaxed);
            let _ = ipc.worker.join();
        }
        if std::mem::take(&mut self.claude_md_injected) {
            strip_session_from_claude_md();
        }
    }
}

impl Drop for RunCleanup {
    fn drop(&mut self) {
        self.release();
    }
}
