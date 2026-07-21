use raios_core::product_factory::{FactoryInvariantError, QualityGateResult};

/// Quality gates return evidence-backed results; they do not waive themselves.
pub trait FactoryGateService {
    fn evaluate_product(
        &self,
        product_id: &str,
    ) -> Result<Vec<QualityGateResult>, FactoryInvariantError>;
}
