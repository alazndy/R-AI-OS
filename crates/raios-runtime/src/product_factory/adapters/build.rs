use raios_core::product_factory::FactoryInvariantError;

pub trait BuildAdapter {
    fn prepare_build(&self, product_id: &str) -> Result<String, FactoryInvariantError>;
}
