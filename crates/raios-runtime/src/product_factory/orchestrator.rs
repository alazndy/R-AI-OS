use raios_core::product_factory::{CycleRecord, FactoryInvariantError, FactoryStage};

/// Owns stage sequencing, never direct shell execution.
pub trait FactoryOrchestrator {
    fn request_stage(
        &self,
        cycle: &CycleRecord,
        stage: FactoryStage,
    ) -> Result<(), FactoryInvariantError>;
}
