use raios_core::product_factory::FactoryInvariantError;

pub trait ResearchAdapter {
    fn research(&self, query_ref: &str) -> Result<String, FactoryInvariantError>;
}
