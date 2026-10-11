//! The terminal view: renders a [`rwk_core::terminal::TerminalEngine`] session with GPUI.

pub mod element;
pub mod gate;
pub mod input;
pub mod metrics;
pub mod registry;
pub mod view;

pub use element::TerminalElement;
pub use gate::FrameGate;
pub use view::{Appearance, FrameStats, TerminalView};
