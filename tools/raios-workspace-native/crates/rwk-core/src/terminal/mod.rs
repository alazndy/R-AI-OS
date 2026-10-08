//! The terminal engine and the PTY host that runs sessions on it.

#[cfg(target_os = "linux")]
pub mod command;
pub mod engine;
pub mod paste;
#[cfg(target_os = "linux")]
pub mod pty;
pub mod snapshot;

#[cfg(target_os = "linux")]
pub use command::{build_command, validate_prompt, ProgramPaths, MAX_PROMPT_CHARS};
pub use engine::{
    CellSide, EngineOptions, EngineStats, ScrollRequest, SelectionKind, TerminalEngine,
};
pub use paste::{encode_paste, MAX_PASTE_BYTES};
#[cfg(target_os = "linux")]
pub use pty::PtyTerminalHost;
pub use snapshot::{Cursor, CursorShape, Line, Modes, RowSpan, Run, Snapshot, Style, TermColor};
