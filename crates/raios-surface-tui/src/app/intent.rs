//! UI user-intent event definitions.

use crate::app::route::Route;
use raios_contracts::{Command, Query};

/// High-level user intent action emitted by keyboard, mouse, or command palette interactions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Switch to a specific route view.
    SwitchRoute(Route),
    /// Cycle to the next route tab.
    NextRoute,
    /// Cycle to the previous route tab.
    PrevRoute,
    /// Move selection cursor up.
    CursorUp,
    /// Move selection cursor down.
    CursorDown,
    /// Move selection cursor left / switch panel focus.
    CursorLeft,
    /// Move selection cursor right / switch panel focus.
    CursorRight,
    /// Toggle panel focus.
    ToggleFocus,
    /// Open the command palette popup.
    OpenCommandPalette,
    /// Close an open modal view or search box.
    CloseModal,
    /// Submit a state-modifying control-plane command.
    SubmitCommand(Command),
    /// Request a read-only query snapshot.
    ExecuteQuery(Query),
    /// Approve a pending handoff request by ID.
    ApproveHandoff(String),
    /// Reject a pending handoff request by ID.
    RejectHandoff(String),
    /// Force a refresh of the typed control-plane snapshot.
    RefreshSnapshot,
    /// Open the help reference screen.
    HelpRequested,
    /// Terminate the application event loop.
    Quit,
}
