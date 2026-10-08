//! The terminal engine and the PTY host that runs sessions on it.

pub mod engine;
pub mod paste;
pub mod snapshot;

pub use engine::{
    CellSide, EngineOptions, EngineStats, ScrollRequest, SelectionKind, TerminalEngine,
};
pub use paste::{encode_paste, MAX_PASTE_BYTES};
pub use snapshot::{Cursor, CursorShape, Line, Modes, RowSpan, Run, Snapshot, Style, TermColor};
