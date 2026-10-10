use rwk_core::terminal::mouse::{encode_mouse, MouseButton, MouseKind, MouseReport};
use rwk_core::terminal::Modes;

fn report(kind: MouseKind, row: u16, col: u16) -> MouseReport {
    MouseReport {
        kind,
        row,
        col,
        shift: false,
        alt: false,
        control: false,
    }
}
fn reporting() -> Modes {
    Modes {
        mouse_report: true,
        sgr_mouse: true,
        ..Modes::default()
    }
}

#[test]
fn a_click_sends_nothing_unless_the_program_asked() {
    let off = Modes::default();
    assert!(encode_mouse(&report(MouseKind::Press(MouseButton::Left), 0, 0), &off).is_none());
    // Asking for reports without the SGR encoding is not supported (decision D4).
    let legacy = Modes {
        mouse_report: true,
        ..Modes::default()
    };
    assert!(encode_mouse(&report(MouseKind::Press(MouseButton::Left), 0, 0), &legacy).is_none());
}

#[test]
fn sgr_press_and_release_use_m_and_uppercase_m() {
    let modes = reporting();
    // Coordinates are one-based on the wire.
    assert_eq!(
        encode_mouse(&report(MouseKind::Press(MouseButton::Left), 4, 9), &modes).unwrap(),
        b"\x1b[<0;10;5M"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::Release(MouseButton::Left), 4, 9), &modes).unwrap(),
        b"\x1b[<0;10;5m"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::Press(MouseButton::Right), 0, 0), &modes).unwrap(),
        b"\x1b[<2;1;1M"
    );
}

#[test]
fn modifiers_and_wheel_set_the_button_bits() {
    let modes = reporting();
    let mut press = report(MouseKind::Press(MouseButton::Left), 0, 0);
    press.control = true; // +16
    press.shift = true; // +4
    assert_eq!(encode_mouse(&press, &modes).unwrap(), b"\x1b[<20;1;1M");
    assert_eq!(
        encode_mouse(&report(MouseKind::WheelUp, 0, 0), &modes).unwrap(),
        b"\x1b[<64;1;1M"
    );
    assert_eq!(
        encode_mouse(&report(MouseKind::WheelDown, 0, 0), &modes).unwrap(),
        b"\x1b[<65;1;1M"
    );
}

#[test]
fn drag_and_motion_need_their_own_modes() {
    let drag = report(MouseKind::Drag(MouseButton::Left), 1, 1);
    let motion = report(MouseKind::Move, 1, 1);
    assert!(encode_mouse(&drag, &reporting()).is_none());
    let with_drag = Modes {
        mouse_drag: true,
        ..reporting()
    };
    assert_eq!(encode_mouse(&drag, &with_drag).unwrap(), b"\x1b[<32;2;2M");
    assert!(encode_mouse(&motion, &with_drag).is_none());
    let with_motion = Modes {
        mouse_motion: true,
        ..reporting()
    };
    assert_eq!(
        encode_mouse(&motion, &with_motion).unwrap(),
        b"\x1b[<35;2;2M"
    );
}
