use raios_contracts::Event;

use crate::app::intent::Intent;
use crate::app::store::Store;

pub fn reduce_intent(store: &mut Store, intent: Intent) {
    match intent {
        Intent::SwitchRoute(r) => {
            store.current_route = r;
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
        }
        Intent::NextRoute => {
            store.current_route = store.current_route.next();
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
        }
        Intent::PrevRoute => {
            store.current_route = store.current_route.prev();
            store.cursor = 0;
            store.sub_cursor = 0;
            store.right_panel_focus = false;
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

pub fn reduce_event(store: &mut Store, event: Event) {
    match event {
        Event::SnapshotUpdated(env) => {
            store.snapshot = *env;
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
    }
}
