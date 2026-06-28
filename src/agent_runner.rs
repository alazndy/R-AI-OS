use crate::instinct::InstinctEngine;
use crate::shield::AgentShield;
use std::io::{self, BufRead};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const BUDGET_LIMIT_KB: u64 = 300;

pub fn run_agent(
    agent: &str,
    project_dir: Option<String>,
    timeout_secs: Option<u64>,
) -> Result<(), String> {
    let shield = AgentShield::init();
    let mut instinct = InstinctEngine::init();

    // 1. Pre-flight Security Check
    if let Some(ref dir) = project_dir {
        let warnings = shield.preflight_check(Path::new(dir));
        for warning in warnings {
            println!("{}", warning);
        }
    }

    // 2. Token Budgeting (Sigmap Integration)
    let mut budget_active = false;
    if let Some(ref dir) = project_dir {
        let size = get_dir_size(Path::new(dir)).unwrap_or(0);
        if size > BUDGET_LIMIT_KB * 1024 {
            println!(
                "📉 Project size ({} KB) exceeds budget ({} KB).",
                size / 1024,
                BUDGET_LIMIT_KB
            );
            println!("🔍 Compacting context via Sigmap...");
            let _ = Command::new("sigmap").current_dir(dir).status();
            budget_active = true;
        }
    }

    // 3. Look up any pending control-plane handoff addressed to this agent identity
    // (see AGENT_CONSTITUTION.md Section 10 — no STATE.json involved). Resolved here,
    // before the Command is built, so each agent's native prompt-injection flag can be
    // used instead of an env var no CLI actually reads.
    let pending_handoff = canonical_agent_identity(agent).and_then(|identity| {
        let conn = crate::db::open_db().ok()?;
        let project_id = project_dir
            .as_deref()
            .and_then(|dir| crate::db::project_id_for_file_path(&conn, dir));
        let ctx = crate::db::cp_take_pending_handoff(&conn, project_id, identity).ok()??;
        let mut block = format!(
            "[HANDOVER CONTEXT]\nFrom: {}  Status: {}\n{}",
            ctx.from_agent, ctx.status, ctx.context_summary
        );
        if let Some(diff_stat) = &ctx.diff_stat {
            block.push_str(&format!("\n\n[Changed files since handoff]\n{diff_stat}"));
        }
        Some((conn, identity, ctx, block))
    });
    let handover_block = pending_handoff.as_ref().map(|(_, _, _, block)| block.clone());

    // 4. Build the Command, wiring the handover into each agent's native prompt flag.
    // NOTE: --append-system-prompt on claude only works with --print (non-interactive).
    // For interactive `raios run claude`, we print the context as a terminal banner
    // and pause for Enter — the user reads the handoff, then the agent starts.
    // This ensures the context is visible in the terminal before the agent TUI takes over.
    if let Some(block) = &handover_block {
        let width = 62usize;
        let border = "═".repeat(width);
        println!("\n\x1b[1;33m╔{border}╗\x1b[0m");
        println!("\x1b[1;33m║\x1b[0m  \x1b[1;33m✦ HANDOVER CONTEXT\x1b[0m{}\x1b[1;33m║\x1b[0m", " ".repeat(width - 20));
        println!("\x1b[1;33m╠{border}╣\x1b[0m");
        for line in block.lines() {
            let truncated: String = line.chars().take(width - 2).collect();
            let pad = width.saturating_sub(truncated.chars().count() + 2);
            println!("\x1b[33m║\x1b[0m  {}{} \x1b[33m║\x1b[0m", truncated, " ".repeat(pad));
        }
        println!("\x1b[1;33m╚{border}╝\x1b[0m");
        println!("\n  \x1b[90mHandoff alındı — devam etmek için \x1b[37m[Enter]\x1b[0m\x1b[90m'a bas...\x1b[0m");
        let stdin = io::stdin();
        let _ = stdin.lock().lines().next();
        println!();
    }

    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("OPENAI_API_KEY");
            // No --append-system-prompt here: that flag implies --print mode and breaks
            // interactive sessions. Context is already printed as a banner above.
            c
        }
        "opencode" => {
            let mut c = Command::new("opencode");
            if let Some(block) = &handover_block {
                c.arg("--prompt").arg(block);
            }
            c
        }
        "cursor" => Command::new("cursor"),
        "antigravity" | "agy" => {
            let mut c = Command::new("agy");
            if let Some(block) = &handover_block {
                c.arg("--prompt-interactive").arg(block);
            }
            c
        }
        "codex" => {
            let mut c = Command::new("codex");
            if let Some(block) = &handover_block {
                c.arg(block);
            }
            c
        }
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    // 5. Inject Instincts & Budget Info
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }
    if budget_active {
        cmd.env("RAIOS_CONTEXT_MODE", "compact");
    }
    // Best-effort env fallback for "cursor" and any future agent without a native flag yet.
    if let Some(block) = &handover_block {
        cmd.env("RAIOS_HANDOVER_CONTEXT", block);
    }

    if let Some(dir) = &project_dir {
        cmd.current_dir(dir);
    }

    // Open a session row in the DB so wrapper-routed sessions are always traceable.
    let session_start_time = SystemTime::now();
    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let session_ids = {
        let identity = canonical_agent_identity(agent).unwrap_or(agent);
        let conn_res = crate::db::open_db();
        match conn_res {
            Ok(conn) => {
                let project_id = project_dir.as_deref()
                    .and_then(|dir| crate::db::project_id_for_file_path(&conn, dir));
                match crate::db::cp_session_start(&conn, identity, project_id) {
                    Ok(ids) => Some((ids.0, ids.1)),
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    };

    // For claude: inject session block into ~/.claude/CLAUDE.md so it's visible
    // inside Claude Code's own UI (system context, /status). Stripped on exit.
    let injected_claude_md = agent.to_lowercase() == "claude";
    if injected_claude_md {
        if let Some((_, run_id)) = &session_ids {
            let identity = canonical_agent_identity(agent).unwrap_or(agent);
            inject_session_to_claude_md(run_id, identity, &now_str);
        }
    }

    // Wrapper-active indicator — always printed so the user can confirm routing is live.
    let session_label = session_ids.as_ref()
        .map(|(_, run_id)| format!("  session: {}", &run_id[..8]))
        .unwrap_or_default();
    println!(
        "\x1b[32m⟦ RAIOS WRAPPER ✓ ⟧\x1b[0m  agent: {}{}",
        canonical_agent_identity(agent).unwrap_or(agent),
        session_label
    );

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn agent: {}", e)),
    };
    if let Some((conn, identity, ctx, _)) = &pending_handoff {
        println!("📨 Handover delivered from {} ({}).", ctx.from_agent, ctx.status);
        if let Err(e) = crate::db::cp_consume_handoff(conn, ctx, identity) {
            eprintln!("Warning: failed to mark handoff as consumed: {e}");
        }
    }

    // 5. Execution & Timeout Loop
    let result = if let Some(timeout) = timeout_secs {
        println!("⏱️ Death timer active: {} seconds.", timeout);
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("✅ Agent ({}) exited: {}", agent, status);
                    if status.success() {
                        break Ok(());
                    } else {
                        break Err(format!("Agent exited with {}", status));
                    }
                }
                Ok(None) => {
                    if start.elapsed().as_secs() > timeout {
                        println!("💀 TIMEOUT! Killing agent ({}) for budget safety...", agent);
                        let _ = child.kill();
                        break Err(format!("Agent {} timed out.", agent));
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                Err(e) => break Err(format!("Process error: {}", e)),
            }
        }
    } else {
        match child.wait() {
            Ok(status) => {
                println!("✅ Agent ({}) exited: {}", agent, status);
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("Agent exited with {}", status))
                }
            }
            Err(e) => Err(format!("Wait error: {}", e)),
        }
    };

    // 6. Close session row in DB and print post-session summary.
    if injected_claude_md {
        strip_session_from_claude_md();
    }
    let success = result.is_ok();
    if let Some((task_id, run_id)) = &session_ids {
        if let Ok(conn) = crate::db::open_db() {
            let _ = crate::db::cp_session_end(&conn, task_id, run_id, success);
        }
    }
    let session_short = session_ids.as_ref()
        .map(|(_, run_id)| format!("  \x1b[90mrun: {}\x1b[0m", &run_id[..8]))
        .unwrap_or_default();
    let identity = canonical_agent_identity(agent).unwrap_or(agent);
    if success {
        println!(
            "\x1b[32m✓ Session ended\x1b[0m{}\n  \x1b[90mHandoff:\x1b[0m raios handoff --to {}-kaira --status success --msg \"...\"",
            session_short, agent
        );
    } else {
        println!(
            "\x1b[31m✗ Session ended (non-zero)\x1b[0m{}\n  \x1b[90mHandoff:\x1b[0m raios handoff --to {}-kaira --status failed --msg \"...\"",
            session_short, identity
        );
    }
    let _ = identity;

    // 7. Post-session Instinct Learning
    if success {
        instinct.data.session_count += 1;
        let _ = instinct.save();
    }

    // 8. For claude: offer to auto-generate a memory.md entry from the session transcript.
    if success && agent.to_lowercase() == "claude" {
        if let Some(ref dir) = project_dir {
            crate::session_memory::post_session_memory_prompt(dir, session_start_time);
        }
    }

    result
}

/// Spawn an agent without waiting for it to exit.
/// Returns the child PID on success.
/// Does NOT do handoff lookup — caller provides the full prompt.
pub fn spawn_agent_detached(
    agent: &str,
    task_prompt: &str,
    project_dir: Option<&str>,
) -> Result<u32, String> {
    let shield = AgentShield::init();
    let instinct = InstinctEngine::init();

    // 1. Pre-flight Security Check
    if let Some(dir) = project_dir {
        let warnings = shield.preflight_check(Path::new(dir));
        for warning in warnings {
            println!("{}", warning);
        }
    }

    // 2. Per-agent command build
    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("OPENAI_API_KEY");
            c.arg("--append-system-prompt").arg(task_prompt);
            c
        }
        "opencode" => {
            let mut c = Command::new("opencode");
            c.arg("--prompt").arg(task_prompt);
            c
        }
        "cursor" => Command::new("cursor"),
        "antigravity" | "agy" => {
            let mut c = Command::new("agy");
            c.arg("--prompt-interactive").arg(task_prompt);
            c
        }
        "codex" => {
            let mut c = Command::new("codex");
            c.arg(task_prompt);
            c
        }
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    // 3. Inject Instincts env
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }

    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }

    println!(
        "🚀 Raios Kernel: Spawning detached agent '{}' under Shield protection...",
        agent
    );

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn agent: {}", e)),
    };

    Ok(child.id())
}

/// Inject a RAIOS session block into ~/.claude/CLAUDE.md so the session ID
/// and tracking info appear inside Claude Code's own UI (visible via /status
/// and referenced in the system context). Strips any stale block first.
fn inject_session_to_claude_md(run_id: &str, agent_identity: &str, started_at: &str) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else { return };
    let stripped = strip_session_block(&content);
    let block = format!(
        "\n<!-- raios-session-begin -->\n\
# RAIOS WRAPPER SESSION\n\
- Session ID: `{}`\n\
- Agent: {}\n\
- Started: {}\n\
- Tracking: `raios sessions` | `raios sessions --agent claude`\n\
<!-- raios-session-end -->\n",
        &run_id[..8],
        agent_identity,
        started_at,
    );
    let _ = std::fs::write(&path, format!("{}{}", stripped.trim_end(), block));
}

/// Remove the RAIOS session block from ~/.claude/CLAUDE.md.
fn strip_session_from_claude_md() {
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = Path::new(&home).join(".claude/CLAUDE.md");
    let Ok(content) = std::fs::read_to_string(&path) else { return };
    let stripped = strip_session_block(&content);
    if stripped != content {
        let _ = std::fs::write(&path, stripped.trim_end().to_string() + "\n");
    }
}

fn strip_session_block(content: &str) -> String {
    const BEGIN: &str = "<!-- raios-session-begin -->";
    const END: &str = "<!-- raios-session-end -->";
    if let (Some(start), Some(end_off)) = (content.find(BEGIN), content.rfind(END)) {
        let before = content[..start].trim_end_matches('\n');
        let after = &content[end_off + END.len()..];
        format!("{}\n{}", before, after.trim_start_matches('\n'))
    } else {
        content.to_string()
    }
}

/// Maps a spawnable agent name to its Kaira identity for handoff lookups.
/// Agents outside the 4-agent matrix (e.g. "cursor") return `None` — they
/// still spawn normally, just without handoff delivery.
fn canonical_agent_identity(agent: &str) -> Option<&'static str> {
    match agent.to_lowercase().as_str() {
        "claude" => Some("claude_kaira"),
        "codex" => Some("codex_kaira"),
        "opencode" => Some("opencode_kaira"),
        "antigravity" | "agy" => Some("antigravity_kaira"),
        _ => None,
    }
}

fn get_dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total_size = 0;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total_size += get_dir_size(&path)?;
            } else {
                total_size += entry.metadata()?.len();
            }
        }
    }
    Ok(total_size)
}
