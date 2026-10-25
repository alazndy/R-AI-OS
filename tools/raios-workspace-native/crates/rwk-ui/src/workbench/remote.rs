//! Closing and killing terminals, and starting remote shells over SSH.
//!
//! "Durdur" ends the process and keeps the pane so its output can still be read. "Kapat" ends
//! it and removes the pane; "Zorla öldür" does the same without the grace an agent's wrapper
//! gets to close its run. Ending a process can wait (an agent has a grace period), so it runs
//! off the UI thread; the pane is gone from the layout at once.

use std::path::PathBuf;

use gpui::{AppContext, Context, Entity, Window};
use rwk_core::sessions::{SessionId, SessionKind, SessionLaunch};
use rwk_core::ssh::SshTarget;
use rwk_core::workbench::CommandId;

use super::{PendingAction, Workbench};
use crate::ssh_picker::{SshPicker, SshPickerEvent};

impl Workbench {
    /// Where `~/.ssh` is looked up. Tests point it at a temporary directory.
    pub fn set_ssh_home(&mut self, home: Option<PathBuf>) {
        self.ssh_home = home;
    }

    /// The open SSH picker.
    pub fn ssh_picker(&self) -> Option<&Entity<SshPicker>> {
        self.ssh_picker.as_ref()
    }

    /// True when the session of terminal `id` is still running.
    pub(super) fn session_running(&self, id: &SessionId) -> bool {
        self.sessions
            .list()
            .iter()
            .any(|session| &session.id == id && session.running)
    }

    /// Closes the selected terminal. A running process is ended after a question; a pane whose
    /// process has already ended is simply removed.
    pub(super) fn request_close_terminal(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.terminals.get(self.selected) else {
            return;
        };
        let id = pane.id.clone();
        if !force && !self.session_running(&id) {
            self.remove_terminal(&id, false, window, cx);
            return;
        }
        self.pending = Some(PendingAction::CloseTerminal { id, force });
        cx.notify();
    }

    /// The cross on a title and a middle click: select that terminal, then close it the same
    /// way the command does.
    pub(super) fn close_terminal_by_id(
        &mut self,
        id: &SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.terminals.iter().position(|pane| &pane.id == id) else {
            return;
        };
        self.select_terminal(index, window, cx);
        self.run(CommandId::CloseTerminal, window, cx);
    }

    /// Removes terminal `id` from the layout and ends its process (`force`: at once). The
    /// process is ended off the UI thread.
    pub(super) fn remove_terminal(
        &mut self,
        id: &SessionId,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.terminals.iter().position(|pane| &pane.id == id) else {
            return;
        };
        let running = self.session_running(id);
        let pane = self.terminals.remove(index);
        self.dock.remove(&pane.id);
        self.gates.detach(&pane.id);
        if self
            .drop_hint
            .as_ref()
            .is_some_and(|(target, _)| target == id)
        {
            self.drop_hint = None;
        }
        let sessions = self.sessions.clone();
        let host = self.host.0.clone();
        let ended = pane.id.clone();
        cx.background_executor()
            .spawn(async move {
                if running {
                    let _ = if force {
                        sessions.kill(&ended)
                    } else {
                        sessions.stop(&ended)
                    };
                }
                host.remove(&ended);
            })
            .detach();
        drop(pane);
        if self.terminals.is_empty() {
            self.selected = 0;
        } else {
            let next = index.min(self.terminals.len() - 1);
            self.select_terminal(next, window, cx);
        }
        self.note_workspace_change(cx);
        self.say(
            if force {
                "Terminal öldürüldü"
            } else {
                "Terminal kapatıldı"
            },
            cx,
        );
        cx.notify();
    }

    // ----- ssh -------------------------------------------------------------------------

    /// Opens the SSH picker over the hosts of `~/.ssh`.
    pub(super) fn open_ssh_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ssh_picker.is_some() {
            return;
        }
        let home = self
            .ssh_home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let hosts = home
            .map(|home| rwk_core::ssh::discover(&home))
            .unwrap_or_default();
        let theme = self.theme();
        let picker = cx.new(|cx| SshPicker::new(hosts, window, cx));
        picker.update(cx, |picker, cx| picker.set_theme(theme, cx));
        self.ssh_subscription = Some(cx.subscribe_in(
            &picker,
            window,
            |this, _picker, event: &SshPickerEvent, window, cx| match event {
                SshPickerEvent::Close => this.close_ssh_picker(window, cx),
                SshPickerEvent::Connect(target) => {
                    let target = target.clone();
                    this.close_ssh_picker(window, cx);
                    this.connect_ssh(target, window, cx);
                }
            },
        ));
        self.ssh_picker = Some(picker);
        cx.notify();
    }

    /// Closes the picker and returns focus to the workbench.
    pub fn close_ssh_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ssh_picker = None;
        self.ssh_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Reconnects the selected SSH shell whose connection has ended: a new shell to the same
    /// target replaces the dead pane, whose output is dropped with it.
    pub(super) fn reconnect_ssh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.terminals.get(self.selected) else {
            return;
        };
        let (Some(target), true) = (pane.ssh.clone(), !self.session_running(&pane.id)) else {
            return;
        };
        let dead = pane.id.clone();
        if self.spawn_terminal(SessionKind::Shell, SessionLaunch::Ssh(target), window, cx) {
            self.remove_terminal(&dead, false, window, cx);
            let last = self.terminals.len().saturating_sub(1);
            self.select_terminal(last, window, cx);
            self.after_started_terminal(window, cx);
        }
    }

    /// Starts a terminal that runs `ssh` to `target`. The pane is named after the target; it is
    /// not part of the saved workspace, so reopening the project never connects by itself.
    pub fn connect_ssh(&mut self, target: SshTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.spawn_terminal(SessionKind::Shell, SessionLaunch::Ssh(target), window, cx) {
            self.after_started_terminal(window, cx);
        }
    }
}
