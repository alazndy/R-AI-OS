pub mod edge;
pub mod evolution;
pub mod instinct;
pub mod router;

// Re-export so existing `crate::edge`, `crate::evolution`,
// `crate::instinct`, `crate::router` paths keep working.
pub use edge::*;
pub use evolution::*;
pub use instinct::*;
pub use router::*;
