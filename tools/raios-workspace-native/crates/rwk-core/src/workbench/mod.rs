//! Workbench logic that does not depend on a toolkit: the command catalog, shortcut resolution,
//! command availability and the open-document session.

pub mod appearance;
pub mod commands;
pub mod dock;
pub mod documents;
pub mod file_types;
pub mod footer;
pub mod layout;
pub mod layout_state;
pub mod menus;
pub mod offers;
pub mod palette;
pub mod quick_open;
pub mod side_diff;
pub mod theme;

pub use appearance::{parse_appearance, serialize_appearance, Appearance, Skin, Tone};
pub use commands::{
    available, catalog, resolve_shortcut, Binding, Command, CommandId, CommandState,
    KeyboardContext, Keystroke, WorkbenchState,
};
pub use dock::{
    zone_at, Axis, DockError, DockLayout, DropZone, Edge, Node, EDGE_BAND, MAX_DEPTH, MIN_SHARE,
};
pub use documents::{DocumentSession, SaveBlocked, SaveTicket, SwitchDecision};
pub use file_types::{presentation, FileIcon, FilePresentation, FileTone};
pub use footer::{footer_items, upsert as upsert_footer_item, FooterInfo, FooterItem, FooterLevel};
pub use layout::{LayoutPreset, MIN_EDITOR_PX, MIN_SIDE_PANE_PX};
pub use layout_state::{
    parse_workspace, serialize_workspace, state_file_name, SavedTab, SavedTaskLink, SavedTerminal,
    SavedWorkspace, WorkspaceStateSource, MAX_SAVED_TABS,
};
pub use menus::{
    is_essential, menu_entries, toolbar, toolbar_icon, toolbar_pressed, MenuEntry, MenuTarget,
    ToolbarGroup, ToolbarItem, ToolbarLayout, ToolbarMode, ToolbarPreset,
};
pub use offers::{
    parse_catalog, plan_offers, MatchedSession, SavedSessionRow, SessionCatalog, SessionOffer,
};
pub use palette::{palette_entries, PaletteEntry};
pub use side_diff::{side_by_side, SideBySide};
pub use theme::Theme;
