use raios_core::product_factory::{CycleRecord, FactoryInvariantError, FactoryPlan};

/// Converts approved product state into bounded control-plane work only after
/// a later implementation phase defines scheduling and approval semantics.
pub trait FactoryPlanner {
    fn materialize_cycle(&self, plan: &FactoryPlan) -> Result<CycleRecord, FactoryInvariantError>;
}
