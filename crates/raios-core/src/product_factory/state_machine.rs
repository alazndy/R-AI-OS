use super::{FactoryInvariantError, ProductStatus};

/// Future transition engines must validate domain state before persistence.
/// The skeleton deliberately exposes no permissive default implementation.
pub trait FactoryStateMachine<State> {
    fn validate_transition(&self, from: State, to: State) -> Result<(), FactoryInvariantError>;
}

/// Product lifecycle transition boundary. Business transition rules are added
/// only after this skeleton receives architectural approval.
pub trait ProductLifecycleStateMachine: FactoryStateMachine<ProductStatus> {}
