use rwk_core::terminal::keys::{encode_key, KeyChord};
use rwk_core::terminal::Modes;

fn chord(key: &str) -> KeyChord {
    KeyChord {
        key: key.to_owned(),
        text: None,
        control: false,
        alt: false,
        shift: false,
    }
}
fn typed(key: &str, text: &str) -> KeyChord {
    KeyChord {
        text: Some(text.to_owned()),
        ..chord(key)
    }
}
fn bytes(chord: &KeyChord, modes: &Modes) -> Vec<u8> {
    encode_key(chord, modes).expect("the chord has a terminal meaning")
}

#[test]
fn printable_text_is_sent_as_utf8() {
    assert_eq!(bytes(&typed("a", "a"), &Modes::default()), b"a");
    assert_eq!(bytes(&typed("ü", "ü"), &Modes::default()), "ü".as_bytes());
}

#[test]
fn control_letters_map_to_c0_codes() {
    let mut c = chord("c");
    c.control = true;
    assert_eq!(bytes(&c, &Modes::default()), vec![0x03]);
    c.key = "[".into();
    assert_eq!(bytes(&c, &Modes::default()), vec![0x1b]);
    c.key = "space".into();
    assert_eq!(bytes(&c, &Modes::default()), vec![0x00]);
}

#[test]
fn alt_prefixes_escape() {
    let mut c = typed("x", "x");
    c.alt = true;
    assert_eq!(bytes(&c, &Modes::default()), b"\x1bx");
}

#[test]
fn arrows_follow_the_application_cursor_mode() {
    let normal = Modes::default();
    let app = Modes {
        application_cursor: true,
        ..Modes::default()
    };
    assert_eq!(bytes(&chord("up"), &normal), b"\x1b[A");
    assert_eq!(bytes(&chord("up"), &app), b"\x1bOA");
    assert_eq!(bytes(&chord("left"), &normal), b"\x1b[D");
}

#[test]
fn modified_arrows_carry_the_xterm_modifier_parameter() {
    let mut c = chord("right");
    c.control = true; // 1 + 4
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;5C");
    c.shift = true; // 1 + 1 + 4
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;6C");
    c.control = false;
    c.shift = false;
    c.alt = true; // 1 + 2
    assert_eq!(bytes(&c, &Modes::default()), b"\x1b[1;3C");
}

#[test]
fn editing_and_function_keys_use_their_standard_sequences() {
    let m = Modes::default();
    assert_eq!(bytes(&chord("enter"), &m), b"\r");
    assert_eq!(bytes(&chord("tab"), &m), b"\t");
    assert_eq!(bytes(&chord("escape"), &m), b"\x1b");
    assert_eq!(bytes(&chord("backspace"), &m), vec![0x7f]);
    assert_eq!(bytes(&chord("delete"), &m), b"\x1b[3~");
    assert_eq!(bytes(&chord("pageup"), &m), b"\x1b[5~");
    assert_eq!(bytes(&chord("home"), &m), b"\x1b[H");
    assert_eq!(bytes(&chord("f1"), &m), b"\x1bOP");
    assert_eq!(bytes(&chord("f5"), &m), b"\x1b[15~");
    assert_eq!(bytes(&chord("f12"), &m), b"\x1b[24~");
    let mut back = chord("tab");
    back.shift = true;
    assert_eq!(bytes(&back, &m), b"\x1b[Z");
}

#[test]
fn a_chord_without_a_terminal_meaning_encodes_to_nothing() {
    let mut c = chord("f20");
    assert!(encode_key(&c, &Modes::default()).is_none());
    c = chord("shift");
    assert!(encode_key(&c, &Modes::default()).is_none());
}
