//! The layout presets moved to the core (`rwk_core::workbench::layout`) because the saved
//! workspace stores one; this re-export keeps every UI path working unchanged.

pub use rwk_core::workbench::layout::{LayoutPreset, MIN_EDITOR_PX, MIN_SIDE_PANE_PX};
