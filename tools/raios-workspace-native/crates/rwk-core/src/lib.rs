//! UI-independent core of RAIOS Workspace Native.
//!
//! Project files and git diffs behind a filesystem capability, terminal sessions behind a
//! host trait, the handoff lifecycle as a pure state machine, a read-only memory projection,
//! UI-local consent, and the client for the `aiosd` control plane.
//!
//! This crate must never depend on a UI toolkit, on `raios-core` or on `raios-runtime`; the
//! control plane is reached only through the `aiosd` contract in `raios-contracts`, and
//! anything that needs `raios-core` (the session token file, secret redaction) is injected by
//! the application crate.

#![warn(missing_docs)]

pub mod consent;
pub mod control;
pub mod documents;
pub mod error;
pub mod handoff_state;
pub mod memory;
pub mod project;
pub mod sessions;

pub use error::{AppError, ErrorCode, Result};
