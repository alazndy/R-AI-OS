//! The saved workspace: what a project had open (tabs, layout, sessions) is handed to the
//! application to store, and read back when the project opens again.
//!
//! Writing is debounced: every change notes the state and only the last one is handed over once
//! the user stops fiddling. A project switch or a close flushes immediately, because those are
//! the moments the file on disk has to be right.

use std::sync::Arc;

use gpui::{Context, Window};
use rwk_core::sessions::{SessionKind, SessionLaunch, MAX_SESSIONS};
use rwk_core::workbench::{SavedTerminal, SavedWorkspace, WorkspaceStateSource};

use super::{Workbench, WORKSPACE_SAVE_DELAY};

impl Workbench {
    /// Connects the saved-workspace source (one state file per project in the application).
    pub fn set_workspace_source(&mut self, source: Option<Arc<dyn WorkspaceStateSource>>) {
        self.workspace_source = source;
    }

    /// The workspace state as it is right now, or `None` while no project is open.
    pub(super) fn workspace_snapshot(&self, cx: &gpui::App) -> Option<SavedWorkspace> {
        let project = self.project.as_ref()?;
        let mut terminals: Vec<SavedTerminal> = self
            .terminals
            .iter()
            .map(|pane| match pane.kind {
                SessionKind::Shell => SavedTerminal::Shell,
                kind => SavedTerminal::Agent {
                    kind,
                    // A pane started from an offer knows the conversation it continues; a fresh
                    // run does not, and is matched by agent when the project opens again.
                    native_session: pane.resume.as_ref().map(|key| key.native_id().to_owned()),
                },
            })
            .collect();
        // An offer that was not taken is offered again next time, with the conversation id it
        // was matched to — the same session comes back, not merely the same agent.
        terminals.extend(self.offers.iter().map(|offer| SavedTerminal::Agent {
            kind: offer.kind,
            native_session: offer.native_session.clone(),
        }));
        Some(SavedWorkspace {
            project: project.info().root.clone(),
            tabs: self.editor.read(cx).tabs_snapshot(cx),
            active: self.editor.read(cx).active_index(),
            layout: self.layout,
            terminals,
        })
    }

    /// Notes that the workspace changed. The state is handed to the application when the write
    /// delay passes, or through [`Self::flush_workspace`] on a switch or a close.
    pub(super) fn note_workspace_change(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.workspace_snapshot(cx) else {
            self.pending_workspace = None;
            return;
        };
        self.pending_workspace = Some(snapshot);
        let timer = cx.background_executor().timer(WORKSPACE_SAVE_DELAY);
        self.workspace_save_task = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |workbench, cx| workbench.flush_workspace(cx))
                .ok();
        }));
    }

    /// Hands the pending state to the application now, if there is one.
    pub(super) fn flush_workspace(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.pending_workspace.take() {
            cx.emit(super::WorkbenchEvent::WorkspaceChanged(workspace));
        }
    }

    /// Applies the stored state of the freshly opened project: layout, editor tabs and shells.
    /// Agent sessions are offered instead of started, which the offer flow does in the dock.
    pub(super) fn restore_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Offers of the previous project die with it, whether or not this project has a state
        // file; a plan still in flight is dropped by the bumped generation.
        self.offers.clear();
        self.resuming.clear();
        self.offers_generation += 1;
        let (Some(project), Some(source)) = (self.project.clone(), self.workspace_source.clone())
        else {
            return;
        };
        let root = project.info().root.clone();
        let Some(saved) = source.load(&root) else {
            return;
        };
        // A state file is trusted only for the project its own name derives from.
        if saved.project != root {
            return;
        }
        self.apply_layout(saved.layout, window, cx);
        self.editor.update(cx, |editor, cx| {
            editor.restore_tabs(Arc::clone(&project), saved.tabs, saved.active, window, cx)
        });
        let shells = saved
            .terminals
            .iter()
            .filter(|terminal| matches!(terminal, SavedTerminal::Shell))
            .take(MAX_SESSIONS)
            .count();
        for _ in 0..shells {
            if !self.spawn_terminal(SessionKind::Shell, SessionLaunch::default(), window, cx) {
                break;
            }
        }
        let agents: Vec<SavedTerminal> = saved
            .terminals
            .iter()
            .filter(|terminal| matches!(terminal, SavedTerminal::Agent { .. }))
            .cloned()
            .collect();
        if !agents.is_empty() {
            self.load_offers(agents, root, window, cx);
        }
    }
}
