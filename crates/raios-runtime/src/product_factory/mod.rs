//! Runtime boundary for Product Factory orchestration.
//!
//! Every item in this module is an interface only. No lifecycle planner,
//! execution path, remote request, or store submission is enabled here.

pub mod adapters;
pub mod artifact_store;
pub mod gates;
pub mod ingest;
pub mod maintenance;
pub mod orchestrator;
pub mod planner;
pub mod service;

pub use adapters::*;
pub use artifact_store::*;
pub use gates::*;
pub use ingest::*;
pub use maintenance::*;
pub use orchestrator::*;
pub use planner::*;
pub use service::*;
