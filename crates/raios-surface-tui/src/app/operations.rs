//! Typed state for the TUI operational-console workflow.
//!
//! This module deliberately models only user intent and selection. Command
//! dispatch stays in the existing authenticated control-plane client, so the
//! console cannot bypass server-side authorization or human approval gates.

/// Sections displayed by the operational console.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OperationPanel {
    /// Items requiring immediate attention, such as approvals and blockers.
    #[default]
    Attention,
    /// The selected project's active work.
    Project,
    /// Contextual next actions available to the operator.
    Actions,
}

impl OperationPanel {
    /// Returns the next focusable console panel, wrapping to the start.
    pub fn next(self) -> Self {
        match self {
            Self::Attention => Self::Project,
            Self::Project => Self::Actions,
            Self::Actions => Self::Attention,
        }
    }
}

/// A safe, named interaction the console can offer to the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationActionKind {
    /// Ask the daemon for a fresh typed snapshot.
    RefreshSnapshot,
    /// Navigate to the selected project's workbench.
    OpenProjectWorkbench,
    /// Inspect the selected approval before resolving it.
    ReviewApproval,
    /// Inspect a task that is currently blocked.
    ReviewBlockedTask,
    /// Start a Codex wrapper session for the selected registered project.
    LaunchCodexAgent,
}

/// One action row in the operational console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationAction {
    /// Stable identifier used only for in-memory selection and tests.
    pub id: &'static str,
    /// Short operator-facing label.
    pub label: &'static str,
    /// Human-readable explanation of the action's scope.
    pub detail: &'static str,
    /// Typed action to resolve through the existing client boundary.
    pub kind: OperationActionKind,
}

/// In-memory selection state for the operational-console surface.
#[derive(Debug, Clone, Default)]
pub struct OperationsConsole {
    /// Currently focused console section.
    pub panel: OperationPanel,
    /// Action selection index within the visible action list.
    pub action_cursor: usize,
    /// Actions derived from the latest trusted snapshot.
    pub actions: Vec<OperationAction>,
}

/// Local draft state for creating a personal task from the WORK route.
#[derive(Debug, Clone, Default)]
pub struct TaskComposer {
    /// Whether the task composer overlay owns keyboard input.
    pub is_open: bool,
    /// Operator-entered task title. Validation remains server-side on submit.
    pub title: String,
    /// Requested task priority, from 0 (lowest) to 255 (highest).
    pub priority: u8,
}

impl TaskComposer {
    /// Opens a fresh composer with the standard personal-task priority.
    pub fn begin(&mut self) {
        self.is_open = true;
        self.title.clear();
        self.priority = 50;
    }

    /// Closes the composer and discards its unsubmitted draft.
    pub fn cancel(&mut self) {
        self.is_open = false;
        self.title.clear();
        self.priority = 50;
    }
}

impl OperationsConsole {
    /// Rebuilds the safe action list from the latest trusted snapshot.
    pub fn rebuild(&mut self, has_approval: bool, has_blocker: bool, has_project: bool) {
        let mut actions = Vec::with_capacity(5);

        if has_approval {
            actions.push(OperationAction {
                id: "review-approval",
                label: "Review pending approval",
                detail: "Focus the highest-priority approval before resolving it.",
                kind: OperationActionKind::ReviewApproval,
            });
        }
        if has_blocker {
            actions.push(OperationAction {
                id: "review-blocker",
                label: "Review blocked task",
                detail: "Focus the first task waiting for operator intervention.",
                kind: OperationActionKind::ReviewBlockedTask,
            });
        }
        if has_project {
            actions.push(OperationAction {
                id: "open-workbench",
                label: "Open project workbench",
                detail: "Inspect selected project tasks, artifacts, and memory posture.",
                kind: OperationActionKind::OpenProjectWorkbench,
            });
            actions.push(OperationAction {
                id: "launch-codex",
                label: "Launch Codex session",
                detail: "Open a tracked interactive Codex wrapper for the selected project.",
                kind: OperationActionKind::LaunchCodexAgent,
            });
        }
        actions.push(OperationAction {
            id: "refresh-snapshot",
            label: "Refresh system snapshot",
            detail: "Request a fresh read-only view from the control plane.",
            kind: OperationActionKind::RefreshSnapshot,
        });

        self.actions = actions;
        self.action_cursor = self.action_cursor.min(self.actions.len().saturating_sub(1));
    }

    /// Returns the action currently selected by the operator, if any.
    pub fn selected_action(&self) -> Option<&OperationAction> {
        self.actions.get(self.action_cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::{OperationActionKind, OperationPanel, OperationsConsole, TaskComposer};

    #[test]
    fn rebuild_preserves_a_safe_bounded_action_selection() {
        let mut console = OperationsConsole {
            panel: OperationPanel::Actions,
            action_cursor: 4,
            actions: Vec::new(),
        };

        console.rebuild(true, false, true);

        assert_eq!(console.panel, OperationPanel::Actions);
        assert_eq!(console.action_cursor, 3);
        assert!(matches!(
            console.selected_action().map(|action| &action.kind),
            Some(OperationActionKind::RefreshSnapshot)
        ));
    }

    #[test]
    fn panel_cycle_covers_every_console_section() {
        assert_eq!(OperationPanel::Attention.next(), OperationPanel::Project);
        assert_eq!(OperationPanel::Project.next(), OperationPanel::Actions);
        assert_eq!(OperationPanel::Actions.next(), OperationPanel::Attention);
    }

    #[test]
    fn task_composer_clears_unsubmitted_input_when_cancelled() {
        let mut composer = TaskComposer {
            is_open: true,
            title: "Unsubmitted task".into(),
            priority: 90,
        };

        composer.cancel();

        assert!(!composer.is_open);
        assert!(composer.title.is_empty());
        assert_eq!(composer.priority, 50);
    }
}
