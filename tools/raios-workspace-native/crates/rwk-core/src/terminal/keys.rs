//! Keyboard input to PTY bytes. Pure: the UI maps its key event to a [`KeyChord`] and the same
//! table serves every toolkit. Reserved workbench shortcuts are filtered before this runs.

use super::snapshot::Modes;

/// One key press as the terminal needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    /// Lower-case key name: a character, or `enter`, `tab`, `escape`, `backspace`, `delete`,
    /// `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `space`,
    /// `f1`..`f12`.
    pub key: String,
    /// The text the key produced under the active layout, when any (IME and AltGr output).
    pub text: Option<String>,
    /// Control is held.
    pub control: bool,
    /// Alt is held.
    pub alt: bool,
    /// Shift is held.
    pub shift: bool,
}

impl KeyChord {
    /// The xterm modifier parameter: 1 + shift + 2*alt + 4*control.
    fn modifier_parameter(&self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control)
    }

    fn has_modifier(&self) -> bool {
        self.shift || self.alt || self.control
    }
}

/// Encodes a key press, or `None` when it has no terminal meaning (a bare modifier, an unknown
/// function key, a control chord with no C0 code).
pub fn encode_key(chord: &KeyChord, modes: &Modes) -> Option<Vec<u8>> {
    if let Some(sequence) = named_key(chord, modes) {
        return Some(sequence);
    }
    if chord.control {
        return control_code(&chord.key).map(|code| with_alt(chord, vec![code]));
    }
    let text = chord.text.as_deref().filter(|text| !text.is_empty())?;
    Some(with_alt(chord, text.as_bytes().to_vec()))
}

fn with_alt(chord: &KeyChord, mut bytes: Vec<u8>) -> Vec<u8> {
    if chord.alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

fn control_code(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let first = chars.next()?;
    if chars.next().is_some() {
        return (key == "space").then_some(0x00);
    }
    match first {
        'a'..='z' => Some(first as u8 - b'a' + 1),
        '@' | ' ' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' | '/' => Some(0x1f),
        _ => None,
    }
}

fn named_key(chord: &KeyChord, modes: &Modes) -> Option<Vec<u8>> {
    let key = chord.key.as_str();
    let modified = chord.has_modifier();
    let parameter = chord.modifier_parameter();
    let cursor = |letter: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{parameter}{letter}").into_bytes()
        } else if modes.application_cursor {
            format!("\x1bO{letter}").into_bytes()
        } else {
            format!("\x1b[{letter}").into_bytes()
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{code};{parameter}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let sequence = match key {
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "enter" => with_alt(chord, vec![b'\r']),
        "tab" if chord.shift => b"\x1b[Z".to_vec(),
        "tab" => with_alt(chord, vec![b'\t']),
        "escape" => with_alt(chord, vec![0x1b]),
        "backspace" => with_alt(chord, vec![0x7f]),
        "f1" => b"\x1bOP".to_vec(),
        "f2" => b"\x1bOQ".to_vec(),
        "f3" => b"\x1bOR".to_vec(),
        "f4" => b"\x1bOS".to_vec(),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        _ => return None,
    };
    Some(sequence)
}
