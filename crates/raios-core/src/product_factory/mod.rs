//! Typed Product Factory domain boundary.
//!
//! This module intentionally contains no planner, executor, integration, or
//! mutation logic. It establishes the durable vocabulary that later phases
//! must use instead of free-form lifecycle strings.

pub mod charter;
pub mod errors;
pub mod impact;
pub mod intake;
pub mod policy;
pub mod quality;
pub mod state_machine;
pub mod types;

pub use charter::*;
pub use errors::*;
pub use impact::*;
pub use intake::*;
pub use policy::*;
pub use quality::*;
pub use state_machine::*;
pub use types::*;
