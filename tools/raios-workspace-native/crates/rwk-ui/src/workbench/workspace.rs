//! The saved workspace: what a project had open (tabs, layout, sessions) is handed to the
//! application to store, and read back when the project opens again.
//!
//! Writing is debounced: every change notes the state and only the last one is handed over once
//! the user stops fiddling. A project switch or a close flushes immediately, because those are
//! the moments the file on disk has to be right.

use std::sync::Arc;

use gpui::{Context, Window};
use rwk_core::sessions::{SessionKind, SessionLaunch, SessionName, MAX_SESSIONS};
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
        // A remote shell is never saved: reopening a project must not connect anywhere.
        let saved_panes: Vec<&super::TerminalPane> = self
            .terminals
            .iter()
            .filter(|pane| pane.ssh.is_none())
            .collect();
        let mut terminals: Vec<SavedTerminal> = saved_panes
            .iter()
            .map(|pane| match pane.kind {
                SessionKind::Shell => SavedTerminal::Shell {
                    name: pane.name.as_ref().map(ToString::to_string),
                },
                kind => SavedTerminal::Agent {
                    kind,
                    name: pane.name.as_ref().map(ToString::to_string),
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
            name: offer.name.as_ref().map(ToString::to_string),
        }));
        let mut task_links: Vec<Option<rwk_core::workbench::SavedTaskLink>> = saved_panes
            .iter()
            .map(|pane| Self::saved_link(&pane.task))
            .collect();
        task_links.extend(self.offers.iter().map(|offer| offer.task.clone()));
        Some(SavedWorkspace {
            task_links,
            project: project.info().root.clone(),
            tabs: self.editor.read(cx).tabs_snapshot(cx),
            active: self.editor.read(cx).active_index(),
            layout: self.layout,
            terminals,
            // Panes are stored as their position among the terminals, which is how they are
            // listed above; a session id means nothing in the next run.
            dock: Some(
                self.dock
                    .filter_map(&|id| saved_panes.iter().position(|pane| &pane.id == id)),
            )
            .filter(|dock| !dock.is_empty()),
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
        let link_of = |index: usize| saved.task_links.get(index).cloned().flatten();
        let shells: Vec<(usize, Option<SessionName>)> = saved
            .terminals
            .iter()
            .enumerate()
            .filter(|(_, terminal)| matches!(terminal, SavedTerminal::Shell { .. }))
            .take(MAX_SESSIONS)
            .map(|(index, terminal)| (index, terminal.name()))
            .collect();
        // Saved position -> the session started for it.
        let mut started: Vec<(usize, rwk_core::sessions::SessionId)> = Vec::new();
        for (saved_index, name) in shells {
            let launch = SessionLaunch::Fresh { prompt: None, name };
            if !self.spawn_terminal(SessionKind::Shell, launch, window, cx) {
                break;
            }
            if let Some(pane) = self.terminals.last_mut() {
                pane.task = Self::live_link(link_of(saved_index));
                started.push((saved_index, pane.id.clone()));
            }
        }
        // The saved arrangement covers the shells that came back; agents rejoin the dock one by
        // one as their offers are taken. Any shell the arrangement does not mention keeps the
        // place the default gave it.
        if let Some(arrangement) = saved.dock.as_ref().map(|dock| {
            dock.filter_map(&|saved_index: &usize| {
                started
                    .iter()
                    .find(|(index, _)| index == saved_index)
                    .map(|(_, id)| id.clone())
            })
        }) {
            if !arrangement.is_empty() {
                self.dock = arrangement;
                for pane in &self.terminals {
                    if !self.dock.contains(&pane.id) {
                        let _ = self.dock.push_default(pane.id.clone());
                    }
                }
            }
        }
        let (agents, links): (Vec<SavedTerminal>, Vec<_>) = saved
            .terminals
            .iter()
            .enumerate()
            .filter(|(_, terminal)| matches!(terminal, SavedTerminal::Agent { .. }))
            .map(|(index, terminal)| (terminal.clone(), link_of(index)))
            .unzip();
        if !agents.is_empty() {
            self.load_offers(agents, links, root, window, cx);
        }
    }
}
