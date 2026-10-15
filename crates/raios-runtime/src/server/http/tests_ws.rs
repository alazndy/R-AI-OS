//! End-to-end coverage for the typed-event path the native workspace consumes:
//! the real HTTP router (routes + auth middleware) served on an ephemeral port
//! with a temporary database, a WebSocket subscriber on `/api/stream`, and
//! control-plane commands issued over HTTP exactly as a client would.
//!
//! No live daemon: `build_router` + `axum::serve` run in-process, and every
//! global touched (HOME, XDG_CONFIG_HOME, CWD, RAIOS_DB_PATH) is redirected
//! under the crate-wide locks for the duration of the test. Unix-only: the
//! session token path resolves through `dirs::config_dir()`, which follows
//! `HOME`/`XDG_CONFIG_HOME` only on Unix (the module gate lives in `mod.rs`,
//! mirroring `auth::middleware_integration_tests`).

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::broadcast;

use raios_contracts::{Command, Query};

use crate::daemon::state::DaemonState;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Redirects `HOME` / `XDG_CONFIG_HOME` / CWD (session token + policy lookups)
/// and `RAIOS_DB_PATH` to a tempdir for one test, holding both crate-wide
/// global-state locks: no other test may observe the redirected environment
/// or install a different typed-event bus while this guard is alive.
struct IsolatedWorkspace {
    _env_lock: std::sync::MutexGuard<'static, ()>,
    _db_lock: std::sync::MutexGuard<'static, ()>,
    original_cwd: std::path::PathBuf,
    original_home: Option<String>,
    original_xdg_config_home: Option<String>,
    tmp: tempfile::TempDir,
}

impl IsolatedWorkspace {
    fn new() -> Self {
        let env_lock = crate::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let db_lock = crate::DB_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().unwrap();
        let original_cwd = std::env::current_dir().unwrap();
        let original_home = std::env::var("HOME").ok();
        let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();

        std::env::set_current_dir(tmp.path()).unwrap();
        std::env::set_var("HOME", tmp.path());
        // dirs::config_dir() prefers XDG_CONFIG_HOME on Linux — must be
        // cleared, or the real session token and policy files leak in.
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::set_var("RAIOS_DB_PATH", tmp.path().join("workspace.db"));

        Self {
            _env_lock: env_lock,
            _db_lock: db_lock,
            original_cwd,
            original_home,
            original_xdg_config_home,
            tmp,
        }
    }

    fn path(&self) -> &std::path::Path {
        self.tmp.path()
    }

    /// Generates a valid session token under the isolated HOME and returns it.
    fn issue_session_token(&self) -> String {
        raios_core::security::SessionTokenManager::new()
            .generate_and_save()
            .unwrap()
    }
}

impl Drop for IsolatedWorkspace {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
        match &self.original_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
        match &self.original_xdg_config_home {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        std::env::remove_var("RAIOS_DB_PATH");
    }
}

/// POSTs a JSON body and parses the JSON response (ureq is blocking; the test
/// runtime stays free for the in-process server).
async fn post_json(url: String, token: String, body: serde_json::Value) -> serde_json::Value {
    tokio::task::spawn_blocking(move || {
        let response = ureq::post(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .unwrap_or_else(|error| panic!("POST {url} failed: {error}"));
        serde_json::from_str::<serde_json::Value>(&response.into_string().unwrap()).unwrap()
    })
    .await
    .unwrap()
}

/// Reads frames until a typed event matches `matching`. Non-matching typed
/// events are buffered (other tests may dispatch control-plane commands on
/// this same process-wide bus) and re-checked first on later calls.
async fn next_typed_event(
    ws: &mut WsStream,
    buffered: &mut Vec<serde_json::Value>,
    matching: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    if let Some(index) = buffered.iter().position(&matching) {
        return buffered.remove(index);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for a matching typed event; buffered: {buffered:?}"
        );
        let frame = tokio::time::timeout(remaining, ws.next())
            .await
            .expect("timed out waiting for a websocket frame")
            .expect("websocket stream ended early")
            .expect("websocket transport error");
        if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if value.get("event_type").is_some() {
                if matching(&value) {
                    return value;
                }
                buffered.push(value);
            }
        }
    }
}

fn read_run_status(run_id: &str) -> String {
    let conn = raios_core::db::open_db().unwrap();
    conn.query_row(
        "SELECT status FROM cp_agent_runs WHERE id = ?1",
        rusqlite::params![run_id],
        |row| row.get(0),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_events_and_task_paths_reach_http_ws_clients() {
    let env = IsolatedWorkspace::new();
    let token = env.issue_session_token();

    // One registered project and one never-registered workspace directory,
    // so both project-path resolution branches cross the real HTTP boundary.
    let registered_dir = env.path().join("demo-project");
    std::fs::create_dir_all(&registered_dir).unwrap();
    let registered_path = registered_dir.to_str().unwrap().to_owned();
    let unregistered_dir = env.path().join("never-registered");
    std::fs::create_dir_all(&unregistered_dir).unwrap();
    let unregistered_path = unregistered_dir.to_str().unwrap().to_owned();
    {
        let conn = raios_core::db::open_db().unwrap();
        conn.execute(
            "INSERT INTO projects (name, path) VALUES ('demo-project', ?1)",
            rusqlite::params![registered_path],
        )
        .unwrap();
    }

    // The daemon bus: installed exactly as the daemon does, and handed to the
    // router so /api/stream forwards whatever control-plane transitions emit.
    let (tx, _) = broadcast::channel::<String>(128);
    crate::events::install(&tx);
    let probe_tx = tx.clone();
    let state = DaemonState::new();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = super::build_router(state, tx);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // WebSocket subscriber on the real /api/stream route (auth included).
    let ws_request = {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = format!("ws://{addr}/api/stream")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse().unwrap());
        request
    };
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_request)
        .await
        .expect("websocket upgrade through the real router must succeed");

    // The upgrade response can reach the client a moment before the
    // server-side handler task actually subscribes to the bus, and broadcast
    // messages are not replayed. Pump probe frames until one arrives: that
    // proves the subscriber is live before any assertion depends on it.
    let warmup_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let _ = probe_tx.send(r#"{"event":"ws-warmup"}"#.to_string());
        match tokio::time::timeout(Duration::from_millis(250), ws.next()).await {
            Ok(Some(Ok(_))) => break,
            Ok(Some(Err(error))) => panic!("websocket error during warmup: {error}"),
            Ok(None) => panic!("websocket closed during warmup"),
            Err(_) => assert!(
                tokio::time::Instant::now() < warmup_deadline,
                "websocket subscriber never attached"
            ),
        }
    }

    let mut buffered: Vec<serde_json::Value> = Vec::new();
    let base = format!("http://{addr}");

    // 1. Two handoffs: one on the registered project, one on a directory that
    // was never registered (the run-contract fallback branch).
    let created_registered = post_json(
        format!("{base}/api/v1/control/command"),
        token.clone(),
        serde_json::to_value(Command::CreateHandoff {
            project_path: registered_path.clone(),
            from_agent: "claude_kaira".into(),
            to_agent: "codex_kaira".into(),
            summary: "Registered workspace handoff".into(),
            idempotency_key: "ws-events-create-registered".into(),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(created_registered["status"], "ok", "{created_registered}");
    let approval_a = created_registered["result"]["approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let run_a = created_registered["result"]["agent_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task_a = created_registered["result"]["task_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let created_unregistered = post_json(
        format!("{base}/api/v1/control/command"),
        token.clone(),
        serde_json::to_value(Command::CreateHandoff {
            project_path: unregistered_path.clone(),
            from_agent: "claude_kaira".into(),
            to_agent: "antigravity_kaira".into(),
            summary: "Unregistered workspace handoff".into(),
            idempotency_key: "ws-events-create-unregistered".into(),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(
        created_unregistered["status"], "ok",
        "{created_unregistered}"
    );
    let approval_b = created_unregistered["result"]["approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let task_b = created_unregistered["result"]["task_id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 2. The work snapshot (real HTTP query) resolves both task roots.
    let snapshot = post_json(
        format!("{base}/api/v1/control/query"),
        token.clone(),
        serde_json::to_value(Query::GetWorkSnapshot).unwrap(),
    )
    .await;
    assert_eq!(snapshot["status"], "ok", "{snapshot}");
    let tasks = snapshot["work"]["tasks"]
        .as_array()
        .expect("work snapshot must contain the task list");
    let task_a_row = tasks
        .iter()
        .find(|task| task["id"].as_str() == Some(task_a.as_str()))
        .expect("registered handoff task must be listed");
    assert_eq!(
        task_a_row["project_path"].as_str(),
        Some(registered_path.as_str())
    );
    assert_eq!(task_a_row["project_name"].as_str(), Some("demo-project"));
    let task_b_row = tasks
        .iter()
        .find(|task| task["id"].as_str() == Some(task_b.as_str()))
        .expect("unregistered handoff task must be listed");
    assert_eq!(
        task_b_row["project_path"].as_str(),
        Some(unregistered_path.as_str()),
        "the run-contract workspace root must resolve the task's project path"
    );
    assert_eq!(task_b_row["project_name"], serde_json::Value::Null);

    // 3. The queued handoff crossed the bus into the websocket.
    let requested = next_typed_event(&mut ws, &mut buffered, |value| {
        value["event_type"] == "ApprovalRequested"
            && value["payload"]["approval_id"].as_str() == Some(approval_a.as_str())
    })
    .await;
    assert_eq!(requested["payload"]["kind"], "handover");
    assert_eq!(requested["payload"]["target"], "codex_kaira");
    assert!(requested["payload"]["title"]
        .as_str()
        .unwrap()
        .contains("Handoff"));
    let run_created = next_typed_event(&mut ws, &mut buffered, |value| {
        value["event_type"] == "AgentRunStateChanged"
            && value["payload"]["run_id"].as_str() == Some(run_a.as_str())
    })
    .await;
    assert_eq!(run_created["payload"]["agent_name"], "claude_kaira");
    assert_eq!(
        run_created["payload"]["status"].as_str(),
        Some(read_run_status(&run_a).as_str()),
        "the emitted run status must mirror the committed row"
    );

    // 4. Approving publishes the resolution plus the run's observed end state.
    let approved = post_json(
        format!("{base}/api/v1/control/command"),
        token.clone(),
        serde_json::to_value(Command::ApproveHandoff {
            approval_id: approval_a.clone(),
            expected_revision: None,
            idempotency_key: "ws-events-approve-a".into(),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(approved["status"], "ok", "{approved}");
    let resolved = next_typed_event(&mut ws, &mut buffered, |value| {
        value["event_type"] == "ApprovalResolved"
            && value["payload"]["approval_id"].as_str() == Some(approval_a.as_str())
    })
    .await;
    assert_eq!(resolved["payload"]["status"], "approved");
    let run_ended = next_typed_event(&mut ws, &mut buffered, |value| {
        value["event_type"] == "AgentRunStateChanged"
            && value["payload"]["run_id"].as_str() == Some(run_a.as_str())
            && value["payload"]["status"] != "awaiting_approval"
    })
    .await;
    assert_eq!(
        run_ended["payload"]["status"].as_str(),
        Some(read_run_status(&run_a).as_str())
    );

    // 5. Rejection is symmetric: same bus, same handler, `rejected` status.
    let rejected = post_json(
        format!("{base}/api/v1/control/command"),
        token.clone(),
        serde_json::to_value(Command::RejectHandoff {
            approval_id: approval_b.clone(),
            expected_revision: None,
            reason: "not needed for this workspace".into(),
            idempotency_key: "ws-events-reject-b".into(),
        })
        .unwrap(),
    )
    .await;
    assert_eq!(rejected["status"], "ok", "{rejected}");
    let resolved_b = next_typed_event(&mut ws, &mut buffered, |value| {
        value["event_type"] == "ApprovalResolved"
            && value["payload"]["approval_id"].as_str() == Some(approval_b.as_str())
    })
    .await;
    assert_eq!(resolved_b["payload"]["status"], "rejected");

    server.abort();
}
