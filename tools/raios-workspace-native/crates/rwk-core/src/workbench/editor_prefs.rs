//! The editor settings that survive a restart: text size, tab width and line wrapping.

use serde::{Deserialize, Serialize};

/// Smallest and largest text size.
pub const MIN_FONT_SIZE: u8 = 9;
/// See [`MIN_FONT_SIZE`].
pub const MAX_FONT_SIZE: u8 = 32;
/// The text size the editor and the terminal start with.
pub const DEFAULT_FONT_SIZE: u8 = 14;

/// Text size, tab width and wrapping of the editor (the text size also drives the terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPrefs {
    /// Text size in pixels.
    pub font_size: u8,
    /// Spaces per tab stop.
    pub tab_size: u8,
    /// Wrap long lines at the window edge.
    pub soft_wrap: bool,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            tab_size: 4,
            soft_wrap: false,
        }
    }
}

impl EditorPrefs {
    /// The same settings with every value inside its limits (a hand-edited file may not be).
    pub fn clamped(self) -> Self {
        Self {
            font_size: self.font_size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE),
            tab_size: self.tab_size.clamp(1, 8),
            soft_wrap: self.soft_wrap,
        }
    }

    /// The text size changed by `delta` pixels, kept inside the limits.
    pub fn zoomed(self, delta: i8) -> Self {
        let size = i16::from(self.font_size) + i16::from(delta);
        Self {
            font_size: size.clamp(i16::from(MIN_FONT_SIZE), i16::from(MAX_FONT_SIZE)) as u8,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_stays_inside_the_limits_and_reset_is_the_default() {
        let prefs = EditorPrefs::default();
        assert_eq!(prefs.zoomed(2).font_size, 16);
        assert_eq!(prefs.zoomed(-100).font_size, MIN_FONT_SIZE);
        assert_eq!(prefs.zoomed(100).font_size, MAX_FONT_SIZE);
        assert_eq!(prefs.zoomed(3).zoomed(-3), prefs);
    }

    #[test]
    fn a_hand_edited_value_outside_the_limits_is_pulled_in() {
        let prefs: EditorPrefs =
            serde_json::from_str(r#"{"font_size": 200, "tab_size": 0}"#).unwrap();
        let prefs = prefs.clamped();
        assert_eq!((prefs.font_size, prefs.tab_size), (MAX_FONT_SIZE, 1));
        assert!(!prefs.soft_wrap, "missing fields take their defaults");
    }
}
