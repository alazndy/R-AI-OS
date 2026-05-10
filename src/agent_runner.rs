use std::process::Command;
use std::time::{Duration, Instant};
use std::thread;
use std::path::Path;
use crate::shield::AgentShield;
use crate::instinct::InstinctEngine;

const BUDGET_LIMIT_KB: u64 = 300;

pub fn run_agent(agent: &str, project_dir: Option<String>, timeout_secs: Option<u64>) -> Result<(), String> {
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
            println!("📉 Project size ({} KB) exceeds budget ({} KB).", size / 1024, BUDGET_LIMIT_KB);
            println!("🔍 Compacting context via Sigmap...");
            let _ = Command::new("sigmap")
                .current_dir(dir)
                .status();
            budget_active = true;
        }
    }

    // 3. Build the Command
    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            c.env_remove("GEMINI_API_KEY");
            c.env_remove("OPENAI_API_KEY");
            c
        },
        "gemini" => {
            let mut c = Command::new("gemini");
            c.env_remove("ANTHROPIC_API_KEY");
            c.env_remove("OPENAI_API_KEY");
            c
        },
        "cursor" => Command::new("cursor"),
        "antigravity" => Command::new("antigravity"),
        _ => return Err(format!("Unsupported agent: {}", agent)),
    };

    // 4. Inject Instincts & Budget Info
    let instinct_prompt = instinct.get_instinct_prompt();
    if !instinct_prompt.is_empty() {
        cmd.env("RAIOS_INSTINCTS", instinct_prompt);
    }
    if budget_active {
        cmd.env("RAIOS_CONTEXT_MODE", "compact");
    }

    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }
    
    println!("🚀 Raios Kernel: Starting agent '{}' under Shield protection...", agent);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Failed to spawn agent: {}", e)),
    };
    
    // 5. Execution & Timeout Loop
    let result = if let Some(timeout) = timeout_secs {
        println!("⏱️ Death timer active: {} seconds.", timeout);
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("✅ Agent ({}) exited: {}", agent, status);
                    if status.success() { break Ok(()); } else { break Err(format!("Agent exited with {}", status)); }
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
                if status.success() { Ok(()) } else { Err(format!("Agent exited with {}", status)) }
            }
            Err(e) => Err(format!("Wait error: {}", e)),
        }
    };

    // 6. Post-session Instinct Learning
    if result.is_ok() {
        instinct.data.session_count += 1;
        let _ = instinct.save();
    }

    result
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
