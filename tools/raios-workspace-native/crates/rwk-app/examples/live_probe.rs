//! Drives `DaemonClient` against a running daemon: reads, policy, registration, handoff, receipt,
//! launch validation. Run it only against an isolated daemon (temporary config and database):
//! the mutating calls are real. `live_probe <project-dir>`; the token is read from
//! `$XDG_CONFIG_HOME/raios/.session_token`.

use std::path::PathBuf;

use rwk_core::control::daemon::{DaemonClient, TokenSource};
use rwk_core::control::{ControlPlane, CreateHandoffRequest, IdempotencyKey, LaunchAgentRequest};
use rwk_core::error::Result;
use rwk_core::sessions::SessionKind;

struct FileToken(PathBuf);
impl TokenSource for FileToken {
    fn bearer_token(&self) -> Result<String> {
        Ok(std::fs::read_to_string(&self.0)
            .unwrap_or_default()
            .trim()
            .to_owned())
    }
}

fn main() {
    let project = PathBuf::from(std::env::args().nth(1).expect("project dir"));
    let config = std::env::var("XDG_CONFIG_HOME").expect("XDG_CONFIG_HOME");
    let client = DaemonClient::new(
        format!("http://127.0.0.1:{}", DaemonClient::DEFAULT_PORT),
        Box::new(FileToken(format!("{config}/raios/.session_token").into())),
    );
    println!(
        "work: {:?}",
        client
            .work_snapshot()
            .map(|w| (w.tasks.len(), w.projects.len()))
    );
    println!(
        "now: {:?}",
        client.now_snapshot().map(|n| n.approvals.len())
    );
    println!("govern: {:?}", client.govern_snapshot().map(|_| "ok"));
    for name in ["launch_agent", "create_handoff", "register_project"] {
        println!("policy {name}: {:?}", client.command_policy(name));
    }
    println!(
        "register: {:?}",
        client.register_project(&project, &IdempotencyKey::fresh())
    );
    let queued = client.create_handoff(&CreateHandoffRequest {
        project_root: project.clone(),
        from: SessionKind::Claude,
        to: SessionKind::Codex,
        summary: "live probe handoff".to_owned(),
        idempotency_key: IdempotencyKey::fresh(),
    });
    println!("handoff: {queued:?}");
    if let Ok(queued) = &queued {
        println!(
            "receipt: {:?}",
            client.handoff_receipt(&queued.approval_id, &project)
        );
    }
    println!(
        "receipt missing: {:?}",
        client
            .handoff_receipt("no-such-approval", &project)
            .map_err(|e| e.code)
    );
    println!(
        "launch validation: {:?}",
        client.launch_agent(&LaunchAgentRequest {
            project_root: project.clone(),
            agent: SessionKind::Codex,
            prompt: None,
            idempotency_key: IdempotencyKey::fresh(),
        })
    );
}
