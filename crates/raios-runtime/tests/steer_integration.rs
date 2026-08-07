//! End-to-end coverage for the tmux-backed spawn -> steer -> captured-output
//! path, exercised at `raios_runtime`'s public crate boundary (real `tmux`
//! process, real pane, real `tmux send-keys` delivery) rather than through
//! `ExecutionProxy`'s unit tests, which reach into `pub(crate)` internals
//! from inside the crate. Requires a real `tmux` binary on PATH — same
//! runtime dependency `raios doctor`'s tmux presence check (Task 1 of this
//! plan) exists to catch.
//!
//! `ExecutionProxy::spawn_via_tmux` is `pub` (widened from `pub(crate)`
//! specifically for this file, see its doc comment in `daemon/proxy.rs`)
//! because `ExecutionProxy::spawn_agent` only accepts one of the four
//! canonical agent identities (`agent_command()` in `proxy.rs`) — there is
//! no way to make it launch a scripted `sh -c '...'` test double. Calling
//! `spawn_via_tmux` directly is the only way to drive a controllable process
//! through the same tmux launch/capture/teardown machinery a real agent
//! spawn uses. `spawn_via_tmux` itself follows a documented find-only
//! contract against `DaemonState.active_agents` (it updates an existing
//! entry's `status`/`logs`, it never inserts one) — `spawn_agent` is the
//! only production caller, and it registers the `AgentProcess` *before*
//! delegating to `spawn_via_tmux`. This test mirrors that same
//! register-then-spawn sequence so `steer_agent`'s "must be a *known,
//! Running* agent" lookup has something to find, matching how the two
//! public methods are actually meant to be composed by a real caller.
use raios_runtime::daemon::proxy::{AgentProcess, ExecutionProxy};
use raios_runtime::daemon::state::DaemonState;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

#[tokio::test]
async fn spawn_then_steer_full_roundtrip() {
    let state = Arc::new(RwLock::new(DaemonState::default()));
    let proxy = ExecutionProxy::new(state.clone());
    let id = uuid::Uuid::new_v4();

    // Mirrors what `spawn_agent` does before delegating to
    // `spawn_via_tmux`: register the agent as "Running" so `steer_agent`
    // (which only steers known, running agents) can find it.
    {
        let mut state_lock = state.write().await;
        state_lock.active_agents.push(AgentProcess {
            id,
            name: "sh".to_string(),
            status: "Running".to_string(),
            started_at: SystemTime::now(),
            logs: Vec::new(),
        });
    }

    proxy
        .spawn_via_tmux(
            id,
            "sh",
            &[
                "-c".to_string(),
                "read line; echo \"GOT:$line\"; sleep 3".to_string(),
            ],
            ".",
            10,
        )
        .await
        .expect("spawn should succeed");

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    proxy
        .steer_agent(id, "integration-test-message", "claude_kaira")
        .await
        .expect("steer should succeed");

    let mut ok = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = state.read().await;
        if let Some(agent) = s.active_agents.iter().find(|a| a.id == id) {
            if agent
                .logs
                .iter()
                .any(|l| l.contains("GOT:integration-test-message"))
            {
                ok = true;
                break;
            }
        }
    }

    // Same cleanup `proxy.rs`'s own `#[tokio::test]`s for this path
    // (`spawn_agent_via_tmux_captures_output_into_logs`,
    // `steer_agent_delivers_via_send_keys_and_is_captured_in_logs`) apply
    // and document: the assertion above wins the race against the pane's
    // own `sleep 3` and the 10s Death Timer, so `#[tokio::test]`'s per-test
    // runtime drops spawn_via_tmux's still-sleeping exit-status/kill-session
    // background task before it reaches its own cleanup. Kill the session
    // (and its pipe-pane logfile) explicitly rather than leaking a live
    // tmux session on every run. Session-name format inlined rather than
    // importing `proxy::tmux_session_name` — that helper is `pub(crate)`
    // and this file is a separate compilation unit, and widening it further
    // is outside this task's scope (only `spawn_via_tmux` was authorized to
    // go from `pub(crate)` to `pub`).
    let session = format!("raios-agent-{id}");
    let _ = tokio::process::Command::new("tmux")
        .args(["kill-session", "-t", &session])
        .status()
        .await;
    let _ = tokio::fs::remove_file(
        std::env::temp_dir()
            .join("raios-agent-logs")
            .join(format!("{session}.log")),
    )
    .await;

    assert!(
        ok,
        "full spawn -> steer -> captured-output roundtrip failed"
    );
}
