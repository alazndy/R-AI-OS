//! Read-only lookup of the policy action the daemon applies to a control command.

use raios_contracts::{CommandPolicyDto, Problem};
use raios_core::security::policy::PolicyAction;
use raios_core::security::PolicyConfig;

const MAX_COMMAND_NAME_CHARS: usize = 64;

fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_COMMAND_NAME_CHARS
        && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
}

/// Resolves the action for `command_name`. `configured` is the rule or default action from
/// the loaded policy, or `None` when no policy file exists, which the dispatcher treats as
/// `confirm`.
pub(crate) fn command_policy(
    command_name: &str,
    configured: Option<&PolicyAction>,
) -> Result<CommandPolicyDto, Problem> {
    if !is_valid_command_name(command_name) {
        return Err(Problem::invalid_input(format!(
            "Command name must be 1 to {MAX_COMMAND_NAME_CHARS} lowercase letters or underscores"
        )));
    }
    Ok(CommandPolicyDto {
        command_name: command_name.to_owned(),
        action: configured
            .map_or("confirm", PolicyAction::as_str)
            .to_owned(),
    })
}

/// Loads the policy the same way the dispatcher does and reports the action for one command.
pub(crate) fn query_command_policy(command_name: &str) -> Result<CommandPolicyDto, Problem> {
    let configured =
        PolicyConfig::try_load_default().map(|policy| policy.tool_action(command_name).clone());
    command_policy(command_name, configured.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_action_is_reported_verbatim() {
        for (configured, expected) in [
            (PolicyAction::Allow, "allow"),
            (PolicyAction::Deny, "deny"),
            (PolicyAction::Confirm, "confirm"),
        ] {
            let dto = command_policy("launch_agent", Some(&configured)).unwrap();
            assert_eq!(dto.command_name, "launch_agent");
            assert_eq!(dto.action, expected);
        }
    }

    #[test]
    fn a_missing_policy_file_means_confirm() {
        let dto = command_policy("create_handoff", None).unwrap();
        assert_eq!(dto.action, "confirm");
    }

    #[test]
    fn malformed_command_names_are_rejected() {
        let too_long = "a".repeat(65);
        for name in [
            "",
            "Launch-Agent",
            "launch agent",
            "launch_agent;",
            too_long.as_str(),
        ] {
            let error = command_policy(name, None).unwrap_err();
            assert_eq!(error.code, "INVALID_INPUT", "{name:?}");
        }
    }
}
