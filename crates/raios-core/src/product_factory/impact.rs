use serde::{Deserialize, Serialize};

use super::{ImpactAssessmentSummary, StorageClass};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactAssessmentRequest {
    pub change_request_id: String,
    pub requested_by: String,
    pub source_revision_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImpactTarget {
    pub target_kind: String,
    pub target_id: String,
    pub storage_class: StorageClass,
}

/// Assessment calculation is intentionally deferred; this trait prevents UI
/// and transport layers from inventing their own impact semantics.
pub trait ImpactAssessmentService {
    fn assess(
        &self,
        request: &ImpactAssessmentRequest,
    ) -> Result<ImpactAssessmentSummary, super::FactoryInvariantError>;
}
