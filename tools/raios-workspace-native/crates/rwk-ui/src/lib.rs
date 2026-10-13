//! The GPUI user interface of the native workspace. Only this crate (and the app crates above
//! it) may depend on GPUI; `rwk-core` stays headless.

pub mod editor;
pub mod file_tree;
pub mod fonts;
pub mod handoff;
pub mod inspector;
pub mod terminal;
pub mod workbench;
