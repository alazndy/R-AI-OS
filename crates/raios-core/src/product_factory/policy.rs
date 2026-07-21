use serde::{Deserialize, Serialize};

use super::MaintenanceMode;

/// Policy values are deliberately explicit so external, paid, and production
/// actions cannot become implicit as Factory capabilities grow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct FactoryPolicy {
    pub require_owner_for_mutation: bool,
    pub require_approval_for_external_actions: bool,
    pub require_approval_for_production_actions: bool,
    pub maintenance_mode: MaintenanceMode,
}

impl Default for FactoryPolicy {
    fn default() -> Self {
        Self {
            require_owner_for_mutation: true,
            require_approval_for_external_actions: true,
            require_approval_for_production_actions: true,
            maintenance_mode: MaintenanceMode::Observe,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the "secure by design" default this module's own doc comment
    /// promises: every gate defaults to the safe (require-approval) side,
    /// and a fresh Factory starts in Observe, not GuardedExecute. If someone
    /// flips one of these to `false`/`GuardedExecute` while adding a new
    /// capability, this test — not a runtime incident — should catch it.
    #[test]
    fn default_policy_is_fail_closed() {
        let policy = FactoryPolicy::default();
        assert!(policy.require_owner_for_mutation);
        assert!(policy.require_approval_for_external_actions);
        assert!(policy.require_approval_for_production_actions);
        assert_eq!(policy.maintenance_mode, MaintenanceMode::Observe);
    }

    #[test]
    fn policy_deserializes_missing_fields_to_fail_closed_defaults() {
        // #[serde(default)] on the struct means a partial/legacy config blob
        // (e.g. an older raios-policy.toml) must still land on the safe
        // defaults for any field it omits, not on Rust's own `Default`
        // (false/0) for that field's type.
        let policy: FactoryPolicy = serde_json::from_str("{}").unwrap();
        assert_eq!(policy, FactoryPolicy::default());
    }
}
