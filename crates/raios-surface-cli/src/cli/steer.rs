/// Rejects an empty steer message before it ever reaches the daemon —
/// mirrors `cli/handoff.rs`'s pattern of failing loud and early on missing
/// required input.
pub(super) fn validate_message(msg: &str) -> Result<(), &'static str> {
    if msg.trim().is_empty() {
        Err("steer message must not be empty")
    } else {
        Ok(())
    }
}

pub(super) fn cmd_steer(agent_id: String, message: String, json: bool) {
    if let Err(e) = validate_message(&message) {
        eprintln!("Steer failed: {e}");
        std::process::exit(1);
    }

    let sender =
        std::env::var("RAIOS_AGENT_IDENTITY").unwrap_or_else(|_| "claude_kaira".into());

    match raios_runtime::daemon_client::steer_agent_via_http(&agent_id, &message, &sender) {
        Ok(_) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"steer": "sent", "agent_id": agent_id}).to_string()
                );
            } else {
                println!("Steer sent to agent {agent_id}");
            }
        }
        Err(e) => {
            eprintln!("Steer failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn steer_requires_nonempty_message() {
        // cmd_steer exits the process on empty input via std::process::exit,
        // matching cmd_handoff's existing "fail loud, fail early" pattern —
        // so this is a plain data-shape test on the validation helper, not
        // a full process test.
        assert!(super::validate_message("").is_err());
        assert!(super::validate_message("hello").is_ok());
    }
}
