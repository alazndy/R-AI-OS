use raios_core::product_factory::FactoryInvariantError;

pub trait StoreAdapter {
    fn prepare_submission(&self, release_id: &str) -> Result<String, FactoryInvariantError>;
}
