use raios_core::product_factory::FactoryInvariantError;

pub trait SupportAdapter {
    fn normalize_item(&self, payload_ref: &str) -> Result<String, FactoryInvariantError>;
}
