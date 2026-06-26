use crate::security::PolicyConfig;
use crate::shield::AgentShield;

// ─── Decision ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum UmaiDecision {
    Allow,
    Deny(String),
    Confirm(String),
}

impl UmaiDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, UmaiDecision::Allow)
    }

    pub fn into_error_json(self) -> serde_json::Value {
        match self {
            UmaiDecision::Deny(reason) => serde_json::json!({
                "event": "UmaiBlocked",
                "reason": reason
            }),
            UmaiDecision::Confirm(reason) => serde_json::json!({
                "event": "UmaiConfirmRequired",
                "reason": reason
            }),
            UmaiDecision::Allow => serde_json::json!({ "event": "UmaiAllow" }),
        }
    }
}

// ─── UMAI ─────────────────────────────────────────────────────────────────────

/// Universal Multi-agent Access & Isolation-guard.
///
/// Single enforcement choke-point wired in front of every WS command dispatch
/// and MCP tool call. Combines PolicyConfig (TOML rule engine) with AgentShield
/// (regex pattern scanner) so neither can be bypassed independently.
pub struct Umai {
    policy: Option<PolicyConfig>,
    shield: AgentShield,
}

impl Umai {
    pub fn new(policy: Option<PolicyConfig>) -> Self {
        Self {
            policy,
            shield: AgentShield::init(),
        }
    }

    /// Load policy from default paths automatically.
    pub fn from_default_policy() -> Self {
        Self::new(PolicyConfig::try_load_default())
    }

    /// Check a WS command name + optional payload string before dispatch.
    ///
    /// Layer order:
    ///   1. AgentShield  — regex scan on any raw string payload
    ///   2. PolicyConfig — per-command allow/deny/confirm rule
    pub fn check(&self, command: &str, payload: Option<&str>) -> UmaiDecision {
        // Layer 1: pattern scan on payload
        if let Some(raw) = payload {
            if !self.shield.is_safe(raw) {
                return UmaiDecision::Deny(format!(
                    "UMAI: payload for '{}' matched a blocked pattern",
                    command
                ));
            }
        }

        // Layer 2: policy rule
        if let Some(ref policy) = self.policy {
            match policy.validate_tool_call(command) {
                Ok(()) => UmaiDecision::Allow,
                Err(e) => {
                    let msg = e.to_string();
                    if msg.starts_with("confirm:") {
                        UmaiDecision::Confirm(msg[8..].to_string())
                    } else {
                        UmaiDecision::Deny(msg)
                    }
                }
            }
        } else {
            UmaiDecision::Allow
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::policy::{FilesystemPolicy, PolicyAction, ToolRule, ToolsPolicy};

    fn policy_with_rules(default: PolicyAction, rules: Vec<ToolRule>) -> PolicyConfig {
        PolicyConfig {
            server: None,
            filesystem: FilesystemPolicy {
                enforce_sandbox: false,
                allowed_paths: vec![],
                blocked_paths: vec![],
            },
            tools: ToolsPolicy { default_action: default, rules },
            egress: None,
            rate_limits: None,
            quarantine: None,
        }
    }

    #[test]
    fn allows_when_no_policy() {
        let umai = Umai::new(None);
        assert_eq!(umai.check("SubmitJob", None), UmaiDecision::Allow);
    }

    #[test]
    fn denies_blocked_tool() {
        let policy = policy_with_rules(
            PolicyAction::Allow,
            vec![ToolRule { name: "run_build".into(), action: PolicyAction::Deny }],
        );
        let umai = Umai::new(Some(policy));
        assert!(matches!(umai.check("run_build", None), UmaiDecision::Deny(_)));
    }

    #[test]
    fn confirm_for_unknown_when_default_is_confirm() {
        let policy = policy_with_rules(PolicyAction::Confirm, vec![]);
        let umai = Umai::new(Some(policy));
        assert!(matches!(umai.check("SomeNewCommand", None), UmaiDecision::Confirm(_)));
    }

    #[test]
    fn shield_blocks_dangerous_payload() {
        let umai = Umai::new(None);
        assert!(matches!(
            umai.check("SubmitJob", Some("rm -rf /")),
            UmaiDecision::Deny(_)
        ));
    }

    #[test]
    fn allow_passes_both_layers() {
        let policy = policy_with_rules(
            PolicyAction::Allow,
            vec![ToolRule { name: "list_projects".into(), action: PolicyAction::Allow }],
        );
        let umai = Umai::new(Some(policy));
        assert_eq!(umai.check("list_projects", Some("safe payload")), UmaiDecision::Allow);
    }
}
