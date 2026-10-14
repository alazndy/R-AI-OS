//! Mouse input to PTY bytes, SGR (1006) encoding only (decision D4).

use super::snapshot::Modes;

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary button.
    Left,
    /// Wheel button.
    Middle,
    /// Secondary button.
    Right,
}

impl MouseButton {
    fn code(self) -> u16 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    }
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    /// A button went down.
    Press(MouseButton),
    /// A button went up.
    Release(MouseButton),
    /// The pointer moved with a button held.
    Drag(MouseButton),
    /// The pointer moved with no button held.
    Move,
    /// The wheel turned away from the user.
    WheelUp,
    /// The wheel turned toward the user.
    WheelDown,
}

/// One mouse event in terminal cell coordinates (zero based).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseReport {
    /// The event.
    pub kind: MouseKind,
    /// Visible row.
    pub row: u16,
    /// Visible column.
    pub col: u16,
    /// Shift is held.
    pub shift: bool,
    /// Alt is held.
    pub alt: bool,
    /// Control is held.
    pub control: bool,
}

/// Encodes a mouse event for the program, or `None` when the program did not ask for it.
pub fn encode_mouse(report: &MouseReport, modes: &Modes) -> Option<Vec<u8>> {
    if !modes.mouse_report || !modes.sgr_mouse {
        return None;
    }
    let (base, release) = match report.kind {
        MouseKind::Press(button) => (button.code(), false),
        MouseKind::Release(button) => (button.code(), true),
        MouseKind::Drag(button) if modes.mouse_drag || modes.mouse_motion => {
            (button.code() + 32, false)
        }
        MouseKind::Move if modes.mouse_motion => (35, false),
        MouseKind::WheelUp => (64, false),
        MouseKind::WheelDown => (65, false),
        MouseKind::Drag(_) | MouseKind::Move => return None,
    };
    let code = base
        + 4 * u16::from(report.shift)
        + 8 * u16::from(report.alt)
        + 16 * u16::from(report.control);
    let final_byte = if release { 'm' } else { 'M' };
    Some(
        format!(
            "\x1b[<{code};{};{}{final_byte}",
            report.col + 1,
            report.row + 1
        )
        .into_bytes(),
    )
}
