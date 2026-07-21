use std::env;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookPayload {
    pub hook: String,
    pub tool_name: Option<String>,
    pub args_json: Option<String>,
    pub project: Option<String>,
    pub agent: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutcome {
    Allowed,
    Blocked { reason: String },
    Skipped,
}

/// Executes a security lifecycle hook script if configured and not recursion-disabled.
/// Exit code 0 = Allowed
/// Exit code 2 = Blocked (reason from stderr or stdout)
/// Any other exit code = fail-open (logs error and allows)
pub fn run_hook(_name: &str, hook_script: &str, payload: &HookPayload) -> HookOutcome {
    if env::var("RAIOS_HOOKS_DISABLED").unwrap_or_default() == "1" {
        return HookOutcome::Skipped;
    }

    let payload_json = match serde_json::to_string(payload) {
        Ok(json) => json,
        Err(_) => return HookOutcome::Allowed,
    };

    // Cap payload size at 16KB
    let payload_bytes = if payload_json.len() > 16 * 1024 {
        payload_json.as_bytes()[..16 * 1024].to_vec()
    } else {
        payload_json.into_bytes()
    };

    let mut command = if cfg!(windows) {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            hook_script,
        ]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", hook_script]);
        command
    };

    let mut child = match command
        .env("RAIOS_HOOKS_DISABLED", "1")
        .env("RAIOS_HOOK_NAME", &payload.hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[Hooks] Failed to spawn hook script '{}': {}",
                hook_script, e
            );
            return HookOutcome::Allowed; // Fail open
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&payload_bytes);
    }

    let output = match child.wait_with_output() {
        Ok(out) => out,
        Err(e) => {
            eprintln!(
                "[Hooks] Error waiting for hook script '{}': {}",
                hook_script, e
            );
            return HookOutcome::Allowed; // Fail open
        }
    };

    match output.status.code() {
        Some(0) => HookOutcome::Allowed,
        Some(2) => {
            let reason_raw = if !output.stderr.is_empty() {
                String::from_utf8_lossy(&output.stderr).to_string()
            } else {
                String::from_utf8_lossy(&output.stdout).to_string()
            };
            let reason = reason_raw.trim().to_string();
            HookOutcome::Blocked {
                reason: if reason.is_empty() {
                    "Blocked by hook script (exit status 2)".to_string()
                } else {
                    reason
                },
            }
        }
        Some(code) => {
            eprintln!(
                "[Hooks] Hook script '{}' failed with exit code {} (fail-open)",
                hook_script, code
            );
            HookOutcome::Allowed
        }
        None => {
            eprintln!(
                "[Hooks] Hook script '{}' terminated by signal (fail-open)",
                hook_script
            );
            HookOutcome::Allowed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // `run_hook` reads the process-global RAIOS_HOOKS_DISABLED env var, and
    // hook_recursion_disabled_returns_skipped below mutates it. std::env
    // mutation is process-wide, not thread-local, and #[test] functions run
    // on parallel threads by default — without this lock, any test here can
    // observe another test's env::set_var mid-flight (flaky: reproduced on
    // macOS CI as `hook_exit_code_zero_is_allowed` seeing Skipped instead of
    // Allowed because the recursion test's set_var raced it).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn hook_exit_code_zero_is_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let payload = HookPayload {
            hook: "pre_tool_call".into(),
            tool_name: Some("test_tool".into()),
            args_json: None,
            project: None,
            agent: None,
            timestamp: "2026-07-15 12:00:00".into(),
        };
        let outcome = run_hook("pre_tool_call", "exit 0", &payload);
        assert_eq!(outcome, HookOutcome::Allowed);
    }

    #[test]
    fn hook_exit_code_two_is_blocked() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let payload = HookPayload {
            hook: "pre_tool_call".into(),
            tool_name: Some("test_tool".into()),
            args_json: None,
            project: None,
            agent: None,
            timestamp: "2026-07-15 12:00:00".into(),
        };
        let script = if cfg!(windows) {
            "[Console]::Error.WriteLine('Security Violation'); exit 2"
        } else {
            "echo 'Security Violation' >&2; exit 2"
        };
        let outcome = run_hook("pre_tool_call", script, &payload);
        assert_eq!(
            outcome,
            HookOutcome::Blocked {
                reason: "Security Violation".to_string()
            }
        );
    }

    #[test]
    fn hook_crash_or_non_two_exit_fails_open() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let payload = HookPayload {
            hook: "pre_tool_call".into(),
            tool_name: Some("test_tool".into()),
            args_json: None,
            project: None,
            agent: None,
            timestamp: "2026-07-15 12:00:00".into(),
        };
        let outcome = run_hook("pre_tool_call", "exit 1", &payload);
        assert_eq!(outcome, HookOutcome::Allowed);
    }

    #[test]
    fn hook_recursion_disabled_returns_skipped() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let original = env::var("RAIOS_HOOKS_DISABLED").ok();
        env::set_var("RAIOS_HOOKS_DISABLED", "1");
        let payload = HookPayload {
            hook: "pre_tool_call".into(),
            tool_name: Some("test_tool".into()),
            args_json: None,
            project: None,
            agent: None,
            timestamp: "2026-07-15 12:00:00".into(),
        };
        let outcome = run_hook("pre_tool_call", "exit 2", &payload);
        match original {
            Some(v) => env::set_var("RAIOS_HOOKS_DISABLED", v),
            None => env::remove_var("RAIOS_HOOKS_DISABLED"),
        }
        assert_eq!(outcome, HookOutcome::Skipped);
    }
}
