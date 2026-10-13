//! Workbench logic that does not depend on a toolkit: the command catalog, shortcut resolution,
//! command availability and the open-document session.

pub mod appearance;
pub mod commands;
pub mod documents;
pub mod file_types;
pub mod palette;
pub mod quick_open;
pub mod theme;

pub use appearance::{parse_appearance, serialize_appearance, Appearance, Skin, Tone};
pub use commands::{
    available, catalog, resolve_shortcut, Binding, Command, CommandId, CommandState,
    KeyboardContext, Keystroke, WorkbenchState,
};
pub use documents::{DocumentSession, SaveBlocked, SaveTicket, SwitchDecision};
pub use file_types::{presentation, FileIcon, FilePresentation, FileTone};
pub use palette::{palette_entries, PaletteEntry};
pub use theme::Theme;
