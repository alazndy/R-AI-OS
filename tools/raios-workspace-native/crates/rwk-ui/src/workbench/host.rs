//! Shares one PTY host between the session registry (which owns a boxed host) and the terminal
//! views (which resize through an `Arc`).

use std::sync::Arc;

use rwk_core::error::Result;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, TerminalHost, TerminalSize};
use rwk_core::terminal::PtyTerminalHost;

/// A cloneable handle on one [`PtyTerminalHost`].
#[derive(Clone)]
pub struct SharedHost(pub Arc<PtyTerminalHost>);

impl TerminalHost for SharedHost {
    fn spawn(
        &self,
        project: &ProjectRoot,
        kind: SessionKind,
        prompt: Option<&str>,
        size: TerminalSize,
    ) -> Result<SessionId> {
        self.0.spawn(project, kind, prompt, size)
    }

    fn write(&self, id: &SessionId, data: &[u8]) -> Result<()> {
        self.0.write(id, data)
    }

    fn resize(&self, id: &SessionId, size: TerminalSize) -> Result<()> {
        self.0.resize(id, size)
    }

    fn stop(&self, id: &SessionId) -> Result<()> {
        self.0.stop(id)
    }

    fn is_running(&self, id: &SessionId) -> bool {
        self.0.is_running(id)
    }
}
