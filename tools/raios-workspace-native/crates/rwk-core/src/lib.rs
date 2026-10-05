//! UI-independent core of RAIOS Workspace Native.
//!
//! This crate is the P1a **skeleton**: types, traits and `unimplemented!("P1b")` bodies that
//! the owner reviews before any business logic is written. It must never depend on a UI
//! toolkit, on `raios-core` or on `raios-runtime`; the control plane is reached only through
//! the `aiosd` contract in `raios-contracts`.

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
