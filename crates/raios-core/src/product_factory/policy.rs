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
