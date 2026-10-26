//! The GPUI user interface of the native workspace. Only this crate (and the app crates above
//! it) may depend on GPUI; `rwk-core` stays headless.

pub mod assets;
pub mod context_menu;
pub mod editor;
pub mod file_tree;
pub mod fonts;
pub mod handoff;
pub mod inspector;
pub mod memory;
pub mod problems;
pub mod rename;
pub mod search;
pub mod ssh_picker;
pub mod terminal;
pub mod workbench;
