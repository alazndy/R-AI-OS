use crate::app::route::Route;
use raios_contracts::{Command, Query};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    SwitchRoute(Route),
    NextRoute,
    PrevRoute,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    ToggleFocus,
    OpenCommandPalette,
    CloseModal,
    SubmitCommand(Command),
    ExecuteQuery(Query),
    ApproveHandoff(String),
    RejectHandoff(String),
    RefreshSnapshot,
    HelpRequested,
    Quit,
}
