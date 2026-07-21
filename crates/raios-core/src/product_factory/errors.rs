use std::fmt::{Display, Formatter};

/// Domain errors reserved for Product Factory invariants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryInvariantError {
    Disabled,
    TransitionEngineNotInstalled,
    RepositoryNotInstalled,
    ArtifactStorageUnavailable,
    ArtifactRejected { reason: String },
    OwnershipRequired,
    ApprovalRequired,
    InvalidStateTransition { from: String, to: String },
}

impl Display for FactoryInvariantError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "Product Factory is disabled"),
            Self::TransitionEngineNotInstalled => {
                write!(f, "Product Factory transition engine is not installed")
            }
            Self::RepositoryNotInstalled => {
                write!(f, "Product Factory repository is not installed")
            }
            Self::ArtifactStorageUnavailable => {
                write!(f, "Product Factory artifact storage is unavailable")
            }
            Self::ArtifactRejected { reason } => {
                write!(f, "Product Factory artifact was rejected: {reason}")
            }
            Self::OwnershipRequired => write!(f, "A workspace owner is required"),
            Self::ApprovalRequired => write!(f, "An explicit approval is required"),
            Self::InvalidStateTransition { from, to } => {
                write!(
                    f,
                    "Invalid Product Factory state transition: {from} -> {to}"
                )
            }
        }
    }
}

impl std::error::Error for FactoryInvariantError {}
