//! Reducer functions applying user intents and daemon events to the store.

use raios_contracts::Event;

use crate::app::intent::Intent;
use crate::app::operations::OperationPanel;
use crate::app::store::{Store, WorkFocus};

/// Reduces a user intent action into updated store state.
pub fn reduce_intent(store: &mut Store, intent: Intent) {
    match intent {
        Intent::SwitchRoute(r) => {
            store.current_route = r;
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
            store.work_focus = WorkFocus::Projects;
            store.operations.panel = OperationPanel::Attention;
            store.operations.action_cursor = 0;
        }
        Intent::NextRoute => {
            store.current_route = store.current_route.next();
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
            store.work_focus = WorkFocus::Projects;
            store.operations.panel = OperationPanel::Attention;
            store.operations.action_cursor = 0;
        }
        Intent::PrevRoute => {
            store.current_route = store.current_route.prev();
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
            store.work_focus = WorkFocus::Projects;
            store.operations.panel = OperationPanel::Attention;
            store.operations.action_cursor = 0;
        }
        Intent::CursorUp => {
            if store.cursor > 0 {
                store.cursor -= 1;
            }
        }
        Intent::CursorDown => {
            store.cursor += 1;
        }
        Intent::CursorLeft => {
            store.right_panel_focus = false;
        }
        Intent::CursorRight => {
            store.right_panel_focus = true;
        }
        Intent::ToggleFocus => {
            store.right_panel_focus = !store.right_panel_focus;
        }
        Intent::OpenCommandPalette => {
            store.command_mode = true;
            store.command_buf.clear();
        }
        Intent::CloseModal => {
            store.command_mode = false;
            store.help_open = false;
        }
        Intent::HelpRequested => {
            store.help_open = !store.help_open;
        }
        Intent::RefreshSnapshot => {
            store.add_log("Snapshot refresh requested...");
        }
        Intent::Quit => {}
        _ => {}
    }
}

/// Reduces an incoming daemon event into updated store state.
pub fn reduce_event(store: &mut Store, event: Event) {
    match event {
        Event::SnapshotUpdated(env) => {
            store.set_snapshot(*env);
            store.daemon_connected = true;
        }
        Event::AgentRunStateChanged {
            agent_name, status, ..
        } => {
            store.add_log(format!("Agent '{}' status: {}", agent_name, status));
        }
        Event::ApprovalRequested { title, target, .. } => {
            store.add_log(format!("Approval requested: {} -> {}", title, target));
        }
        Event::ApprovalResolved {
            approval_id,
            status,
            ..
        } => {
            store.add_log(format!("Approval {} resolved: {}", approval_id, status));
        }
        Event::LogAppended { log } => {
            store.add_log(format!("[{}] {}", log.category, log.message));
        }
        Event::CommandFailed { problem, .. } => {
            store.last_error = Some(problem.message.clone());
            store.add_log(format!("ERROR [{}]: {}", problem.code, problem.message));
        }
        Event::CommandSucceeded {
            idempotency_key, ..
        } => {
            store.add_log(format!("Command accepted: {idempotency_key}"));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::route::Route;
    use raios_contracts::SnapshotEnvelope;

    #[test]
    fn reduce_route_switch() {
        let mut store = Store::new();
        assert_eq!(store.current_route, Route::Now);

        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Work);

        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Explore);

        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Govern);

        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Now);
    }

    #[test]
    fn reduce_snapshot_event() {
        let mut store = Store::new();
        let env = SnapshotEnvelope {
            sequence: 42,
            timestamp: "2026-07-15T12:00:00Z".into(),
            now: Default::default(),
            work: Default::default(),
            explore: Default::default(),
            govern: Default::default(),
        };

        reduce_event(&mut store, Event::SnapshotUpdated(Box::new(env.clone())));
        assert_eq!(store.snapshot.sequence, 42);
        assert!(store.daemon_connected);
    }

    #[test]
    fn switch_route_resets_navigation_state() {
        let mut store = Store::new();
        store.cursor = 5;
        store.sub_cursor = 7;
        store.right_panel_focus = true;
        store.work_focus = WorkFocus::Tasks;
        store.operations.panel = OperationPanel::Project;
        store.operations.action_cursor = 3;

        reduce_intent(&mut store, Intent::SwitchRoute(Route::Govern));

        assert_eq!(store.current_route, Route::Govern);
        assert_eq!(store.cursor, 0);
        assert_eq!(store.sub_cursor, 0);
        assert!(!store.right_panel_focus);
        assert_eq!(store.work_focus, WorkFocus::Projects);
        assert_eq!(store.operations.panel, OperationPanel::Attention);
        assert_eq!(store.operations.action_cursor, 0);
    }

    #[test]
    fn next_and_prev_route_cycle_through_all_routes() {
        let mut store = Store::new();
        assert_eq!(store.current_route, Route::Now);
        reduce_intent(&mut store, Intent::PrevRoute);
        assert_eq!(store.current_route, Route::Govern);
        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Now);
        reduce_intent(&mut store, Intent::NextRoute);
        assert_eq!(store.current_route, Route::Work);
    }

    #[test]
    fn cursor_movement_handles_clamping_and_focus() {
        let mut store = Store::new();
        reduce_intent(&mut store, Intent::CursorUp);
        assert_eq!(store.cursor, 0, "cursor must not go below zero");

        reduce_intent(&mut store, Intent::CursorDown);
        assert_eq!(store.cursor, 1);
        reduce_intent(&mut store, Intent::CursorDown);
        assert_eq!(store.cursor, 2);

        reduce_intent(&mut store, Intent::CursorLeft);
        assert!(!store.right_panel_focus);
        reduce_intent(&mut store, Intent::CursorRight);
        assert!(store.right_panel_focus);
        reduce_intent(&mut store, Intent::ToggleFocus);
        assert!(!store.right_panel_focus);
    }

    #[test]
    fn command_palette_open_and_close_modal() {
        let mut store = Store::new();
        store.command_buf = "stale".into();
        reduce_intent(&mut store, Intent::OpenCommandPalette);
        assert!(store.command_mode);
        assert!(store.command_buf.is_empty());

        store.help_open = true;
        reduce_intent(&mut store, Intent::CloseModal);
        assert!(!store.command_mode);
        assert!(!store.help_open);

        reduce_intent(&mut store, Intent::HelpRequested);
        assert!(store.help_open);
        reduce_intent(&mut store, Intent::HelpRequested);
        assert!(!store.help_open);
    }

    #[test]
    fn refresh_snapshot_and_quit() {
        let mut store = Store::new();
        reduce_intent(&mut store, Intent::RefreshSnapshot);
        assert!(store.logs.iter().any(|l| l.contains("Snapshot refresh")));
        reduce_intent(&mut store, Intent::Quit);
    }

    #[test]
    fn agent_run_event_appends_status_log() {
        let mut store = Store::new();
        reduce_event(
            &mut store,
            Event::AgentRunStateChanged {
                run_id: "run-1".into(),
                agent_name: "claude".into(),
                status: "running".into(),
            },
        );
        assert!(store
            .logs
            .iter()
            .any(|l| l.contains("claude") && l.contains("running")));
    }

    #[test]
    fn approval_events_append_logs() {
        let mut store = Store::new();
        reduce_event(
            &mut store,
            Event::ApprovalRequested {
                approval_id: "a-1".into(),
                kind: "handoff".into(),
                title: "Approve handoff".into(),
                target: "codex".into(),
            },
        );
        assert!(store
            .logs
            .iter()
            .any(|l| l.contains("Approve handoff") && l.contains("codex")));

        reduce_event(
            &mut store,
            Event::ApprovalResolved {
                approval_id: "a-1".into(),
                status: "approved".into(),
            },
        );
        assert!(store
            .logs
            .iter()
            .any(|l| l.contains("a-1") && l.contains("approved")));
    }

    #[test]
    fn log_appended_event_prefixes_category() {
        let mut store = Store::new();
        reduce_event(
            &mut store,
            Event::LogAppended {
                log: raios_contracts::LogEntryDto {
                    timestamp: "2026-07-15T12:00:00Z".into(),
                    category: "security".into(),
                    message: "file scan completed".into(),
                },
            },
        );
        assert!(store
            .logs
            .iter()
            .any(|l| l.contains("[security] file scan completed")));
    }

    #[test]
    fn command_failure_sets_last_error() {
        let mut store = Store::new();
        reduce_event(
            &mut store,
            Event::CommandFailed {
                idempotency_key: "k-1".into(),
                problem: raios_contracts::Problem {
                    code: "UNAUTHORIZED".into(),
                    message: "token expired".into(),
                    details: None,
                    retryable: false,
                },
            },
        );
        assert_eq!(store.last_error.as_deref(), Some("token expired"));
        assert!(store
            .logs
            .iter()
            .any(|l| l.contains("UNAUTHORIZED") && l.contains("token expired")));
    }

    #[test]
    fn command_success_appends_idempotency_log() {
        let mut store = Store::new();
        reduce_event(
            &mut store,
            Event::CommandSucceeded {
                idempotency_key: "k-2".into(),
                result: None,
            },
        );
        assert!(store.logs.iter().any(|l| l.contains("k-2")));
    }

    #[test]
    fn unrelated_events_are_ignored() {
        let mut store = Store::new();
        store.logs.clear();
        reduce_event(&mut store, Event::HealthDeltaUpdated { reports: vec![] });
        assert!(store.logs.is_empty());
    }
}
