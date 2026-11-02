//! Editor colours: the token-class resolver and the editor style for each tone. Syntax colours
//! are chosen here, never by a grammar's own theme, so switching tone re-resolves instead of
//! re-parsing.

use std::sync::Arc;

use gpui::{rgb, HighlightStyle, Hsla};
use gpui_base::input::{DiagnosticColors, HighlightStyleResolver, InputEditorStyle};
use rwk_core::workbench::{Skin, Theme, Tone};

/// Resolves token-class and diff names to colours.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxStyles {
    tone: Tone,
    aperture: bool,
}

impl SyntaxStyles {
    /// The resolver for `tone`.
    pub fn new(tone: Tone) -> Self {
        Self {
            tone,
            aperture: false,
        }
    }

    /// The resolver for `theme`: the Aperture skin has its own dark colours.
    pub fn themed(theme: Theme) -> Self {
        Self {
            tone: theme.tone(),
            aperture: theme.skin == Skin::Aperture,
        }
    }

    /// The colour of a class name, `None` for names this editor does not colour.
    pub fn color(&self, name: &str) -> Option<u32> {
        if self.aperture && self.tone == Tone::Dark {
            let hex = match name {
                "keyword" => 0xff740d,
                "string" | "number" => 0xffa860,
                "comment" => 0x6b7580,
                "type" | "constant" => 0x7fd0ff,
                "function" | "attribute" => 0x3aa9f0,
                "punctuation" => 0xd0d0d0,
                "property" => 0xf2f2f2,
                "tag" => 0xff9a4d,
                "diff_added" => 0x3ddc84,
                "diff_removed" => 0xff4d4d,
                "diff_hunk" => 0x3aa9f0,
                "diff_header" => 0xc58aff,
                _ => return None,
            };
            return Some(hex);
        }
        let (dark, light) = match name {
            "keyword" => (0xc586c0, 0x8f3f9f),
            "string" => (0xce9178, 0xa31515),
            "comment" => (0x6a9955, 0x4f7a3a),
            "number" => (0xb5cea8, 0x1f7a4d),
            "type" => (0x4ec9b0, 0x1f7a7a),
            "function" => (0xdcdcaa, 0x795e26),
            "constant" => (0x4fc1ff, 0x0b5fa5),
            "punctuation" => (0x9a9a9a, 0x5a5a5a),
            "property" => (0x9cdcfe, 0x0451a5),
            "tag" => (0x569cd6, 0x800000),
            "attribute" => (0xd7ba7d, 0x946b00),
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
    let aperture = theme.skin == Skin::Aperture && tone == Tone::Dark;
    let (selection, caret, active_line) = match tone {
        Tone::Dark if aperture => (0x4a2a0a, 0xff740d, 0x0e0e0e),
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
        highlight_styles: Arc::new(SyntaxStyles::themed(theme)),
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

    const CLASSES: [&str; 15] = [
        "keyword",
        "string",
        "comment",
        "number",
        "type",
        "function",
        "constant",
        "punctuation",
        "property",
        "tag",
        "attribute",
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

    #[test]
    fn the_aperture_skin_colours_keywords_orange_and_covers_every_class() {
        use rwk_core::workbench::Appearance;
        let theme = Theme::of(Appearance {
            skin: Skin::Aperture,
            tone: Tone::Dark,
        });
        let styles = SyntaxStyles::themed(theme);
        assert_eq!(styles.color("keyword"), Some(0xff740d));
        for class in CLASSES {
            assert!(styles.color(class).is_some(), "{class}");
        }
        // The other skins keep their own colours.
        let standard = Theme::of(Appearance::default());
        assert_eq!(
            SyntaxStyles::themed(standard).color("keyword"),
            Some(0xc586c0)
        );
    }
}
