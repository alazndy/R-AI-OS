use raios_core::product_factory::{FactoryInvariantError, SupportItemRecord};

/// External sources remain untrusted until a later ingestion implementation
/// validates provenance, ownership, and payload shape.
pub trait FactoryIngestService {
    fn ingest_support_item(
        &self,
        source_kind: &str,
        payload_ref: &str,
    ) -> Result<SupportItemRecord, FactoryInvariantError>;
}
