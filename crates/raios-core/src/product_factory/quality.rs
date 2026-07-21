use serde::{Deserialize, Serialize};

use super::QualityCheckStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityProfile {
    pub id: String,
    pub product_id: String,
    pub name: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityGateResult {
    pub profile_id: String,
    pub status: QualityCheckStatus,
    pub evidence_ref: Option<String>,
}

pub trait QualityGateService {
    fn evaluate(
        &self,
        product_id: &str,
    ) -> Result<Vec<QualityGateResult>, super::FactoryInvariantError>;
}
