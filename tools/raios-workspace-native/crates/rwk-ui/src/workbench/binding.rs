//! The link between a terminal and the task given to its agent.
//!
//! The assignment itself lives in the daemon (the task's assignee is the agent's short name);
//! the pane only remembers which task it was given so the dock can show it, the workspace file
//! can keep it, and the task list can say when the terminal went away. Closing the terminal
//! does not change the task: the owner decides what happens to it.

use gpui::Context;
use rwk_core::handoff_flow::TaskLink;
use rwk_core::sessions::SessionId;
use rwk_core::workbench::SavedTaskLink;

use super::Workbench;

impl Workbench {
    /// The task terminal `index` carries.
    pub fn terminal_task(&self, index: usize) -> Option<&TaskLink> {
        self.terminals.get(index)?.task.as_ref()
    }

    /// Remembers that task `task_id` was sent to the agent of terminal `id`; the link is made
    /// when the daemon accepts the assignment.
    pub(super) fn begin_bind(&mut self, task_id: String, id: SessionId) {
        self.pending_bind = Some((task_id, id));
    }

    /// The daemon answered the assignment of `task_id`: `assigned` is true when it now has an
    /// assignee, false when the assignee was cleared. Anything but a success drops the pending
    /// link.
    pub(super) fn finish_bind(&mut self, task_id: &str, assigned: bool, cx: &mut Context<Self>) {
        let pending = self.pending_bind.take();
        // A cleared assignee frees whichever terminal carried the task.
        for pane in &mut self.terminals {
            if pane
                .task
                .as_ref()
                .is_some_and(|task| task.task_id == task_id)
            {
                pane.task = None;
            }
        }
        if let (true, Some((pending_id, session))) = (assigned, pending) {
            if pending_id == task_id {
                let title = self
                    .inspector
                    .read(cx)
                    .model()
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .map(|task| task.title.clone())
                    .unwrap_or_else(|| task_id.to_owned());
                if let Some(pane) = self.terminals.iter_mut().find(|pane| pane.id == session) {
                    pane.task = Some(TaskLink {
                        task_id: task_id.to_owned(),
                        title,
                    });
                }
            }
        }
        self.closed_tasks.remove(task_id);
        self.sync_task_marks(cx);
        self.note_workspace_change(cx);
        cx.notify();
    }

    /// An assignment failed: nothing is linked.
    pub(super) fn abandon_bind(&mut self) {
        self.pending_bind = None;
    }

    /// The terminal that carried a task is gone; the task list says so.
    pub(super) fn mark_terminal_closed(&mut self, task: Option<TaskLink>, cx: &mut Context<Self>) {
        if let Some(task) = task {
            self.closed_tasks.insert(task.task_id);
            self.sync_task_marks(cx);
        }
    }

    /// Pushes the tasks whose terminal closed to the task list.
    pub(super) fn sync_task_marks(&mut self, cx: &mut Context<Self>) {
        let closed = self.closed_tasks.clone();
        self.inspector
            .update(cx, |inspector, cx| inspector.set_closed_tasks(closed, cx));
    }

    /// The saved form of a link.
    pub(super) fn saved_link(task: &Option<TaskLink>) -> Option<SavedTaskLink> {
        task.as_ref().map(|task| SavedTaskLink {
            id: task.task_id.clone(),
            title: task.title.clone(),
        })
    }

    /// The live form of a saved link.
    pub(super) fn live_link(saved: Option<SavedTaskLink>) -> Option<TaskLink> {
        saved.map(|saved| TaskLink {
            task_id: saved.id,
            title: saved.title,
        })
    }
}
