//! Editor colours: the token-class resolver and the editor style for each tone. Syntax colours
//! are chosen here, never by a grammar's own theme, so switching tone re-resolves instead of
//! re-parsing.

use std::sync::Arc;

use gpui::{rgb, HighlightStyle, Hsla};
use gpui_base::input::{DiagnosticColors, HighlightStyleResolver, InputEditorStyle};
use rwk_core::workbench::{Theme, Tone};

/// Resolves token-class and diff names to colours.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxStyles {
    tone: Tone,
}

impl SyntaxStyles {
    /// The resolver for `tone`.
    pub fn new(tone: Tone) -> Self {
        Self { tone }
    }

    /// The colour of a class name, `None` for names this editor does not colour.
    pub fn color(&self, name: &str) -> Option<u32> {
        let (dark, light) = match name {
            "keyword" => (0xc586c0, 0x8f3f9f),
            "string" => (0xce9178, 0xa31515),
            "comment" => (0x6a9955, 0x4f7a3a),
            "number" => (0xb5cea8, 0x1f7a4d),
            "type" => (0x4ec9b0, 0x1f7a7a),
            "function" => (0xdcdcaa, 0x795e26),
            "constant" => (0x4fc1ff, 0x0b5fa5),
            "punctuation" => (0x9a9a9a, 0x5a5a5a),
            "diff_added" => (0x3fb950, 0x1a7f37),
            "diff_removed" => (0xf85149, 0xcf222e),
            "diff_hunk" => (0x58a6ff, 0x0969da),
            "diff_header" => (0xd2a8ff, 0x8250df),
            _ => return None,
        };
        Some(match self.tone {
            Tone::Dark => dark,
            Tone::Light => light,
        })
    }
}

impl HighlightStyleResolver for SyntaxStyles {
    fn style(&self, name: &str) -> Option<HighlightStyle> {
        self.color(name).map(|hex| HighlightStyle {
            color: Some(Hsla::from(rgb(hex))),
            ..HighlightStyle::default()
        })
    }
}

/// The editor style for a theme: surface and text colours come from the skin, syntax colours
/// from the tone.
pub fn editor_style(theme: Theme) -> InputEditorStyle {
    let tone = theme.tone();
    let (selection, caret, active_line) = match tone {
        Tone::Dark => (0x264f78, 0xffffff, 0x2a2d2e),
        Tone::Light => (0xadd6ff, 0x000000, 0xf3f5f7),
    };
    let color = |hex: u32| Hsla::from(rgb(hex));
    InputEditorStyle {
        foreground: color(theme.text),
        muted_foreground: color(theme.muted),
        background: color(theme.editor),
        border: color(theme.line),
        selection: color(selection),
        caret: color(caret),
        diagnostics: DiagnosticColors {
            error: color(0xf14c4c),
            warning: color(0xcca700),
            info: color(0x3794ff),
            hint: color(0x858585),
        },
        highlight_styles: Arc::new(SyntaxStyles::new(tone)),
        editor_invisible: Some(color(0x3b3b3b)),
        editor_active_line: Some(color(if theme.lcars {
            theme.raised
        } else {
            active_line
        })),
        editor_gutter_background: Some(color(theme.editor)),
        fold_icon_renderer: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSES: [&str; 12] = [
        "keyword",
        "string",
        "comment",
        "number",
        "type",
        "function",
        "constant",
        "punctuation",
        "diff_added",
        "diff_removed",
        "diff_hunk",
        "diff_header",
    ];

    #[test]
    fn every_class_has_a_colour_in_both_tones() {
        for tone in [Tone::Dark, Tone::Light] {
            let styles = SyntaxStyles::new(tone);
            for class in CLASSES {
                assert!(styles.color(class).is_some(), "{class} {tone:?}");
                assert!(styles.style(class).is_some());
            }
            assert!(styles.color("not-a-class").is_none());
        }
    }

    #[test]
    fn the_tones_differ_so_switching_changes_the_colours() {
        let dark = SyntaxStyles::new(Tone::Dark);
        let light = SyntaxStyles::new(Tone::Light);
        for class in CLASSES {
            assert_ne!(dark.color(class), light.color(class), "{class}");
        }
    }
}
