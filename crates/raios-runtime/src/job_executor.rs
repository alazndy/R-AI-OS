//! Compatibility name for the legacy asynchronous job queue.
//!
//! `Factory` historically meant a background shell-job queue. Product Factory
//! is a separate lifecycle domain, so new internal callers should use
//! `JobExecutor` while compatibility callers continue to compile unchanged.

pub use crate::factory::{Factory as JobExecutor, Job, JobStatus, TaskFn};
