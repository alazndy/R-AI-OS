use raios_core::product_factory::{FactoryInvariantError, MaintenanceMode, SupportItemRecord};

/// Support and maintenance are policy-gated from the first implementation.
pub trait FactoryMaintenanceService {
    fn process_support_item(
        &self,
        item: &SupportItemRecord,
        mode: MaintenanceMode,
    ) -> Result<(), FactoryInvariantError>;
}
