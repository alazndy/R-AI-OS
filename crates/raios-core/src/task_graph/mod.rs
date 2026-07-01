mod dag;
mod store;
pub mod types;

pub(crate) const MAX_NODES: usize = 50;

pub use store::GraphStore;
pub use types::{GraphNode, NodeSpec, NodeStatus, TaskGraph};

#[cfg(test)]
mod tests;
