//! Colour tokens of the four appearances (skin × tone), taken from Workspace 0.1's style sheets.
//! Pure data: the UI turns these into GPUI colours. The text colours keep the 0.1 contrast gates
//! (see the tests).

use super::appearance::{Appearance, Skin, Tone};

/// Every colour the chrome uses, as 0xRRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// True for the GT-LCARS skin: segmented rails, rounded bars, the Antonio display font.
    pub lcars: bool,
    /// True for a dark tone.
    pub dark: bool,
    /// Window background.
    pub bg: u32,
    /// Side panels.
    pub panel: u32,
    /// Editor and terminal surface.
    pub editor: u32,
    /// Raised surfaces, hover.
    pub raised: u32,
    /// Hairlines.
    pub line: u32,
    /// Primary text.
    pub text: u32,
    /// Secondary text.
    pub muted: u32,
    /// Accent fill.
    pub accent: u32,
    /// Accent used as text.
    pub accent_ink: u32,
    /// Blue fill.
    pub blue: u32,
    /// Blue used as text.
    pub blue_ink: u32,
    /// Light blue.
    pub ice: u32,
    /// Selected row.
    pub selected: u32,
    /// Success text.
    pub good: u32,
    /// Error text.
    pub bad: u32,
    /// Text on top of an accent or blue fill.
    pub on_fill: u32,
}

impl Theme {
    /// The tokens of `appearance`.
    pub fn of(appearance: Appearance) -> Self {
        match (appearance.skin, appearance.tone) {
            (Skin::Standard, Tone::Dark) => Self {
                lcars: false,
                dark: true,
                bg: 0x17191b,
                panel: 0x1d2022,
                editor: 0x191c1e,
                raised: 0x292d30,
                line: 0x373d41,
                text: 0xe4e7e8,
                muted: 0xabb3b8,
                accent: 0xe4b77c,
                accent_ink: 0xe4b77c,
                blue: 0x96b9df,
                blue_ink: 0x96b9df,
                ice: 0xc4daf1,
                selected: 0x353025,
                good: 0xa6ccaa,
                bad: 0xe6a6a5,
                on_fill: 0x191c1e,
            },
            (Skin::Standard, Tone::Light) => Self {
                lcars: false,
                dark: false,
                bg: 0xf5f4f1,
                panel: 0xeeefed,
                editor: 0xfafaf7,
                raised: 0xe4e6e3,
                line: 0xc3c8c3,
                text: 0x232b2c,
                muted: 0x586367,
                accent: 0xdcb778,
                accent_ink: 0x775010,
                blue: 0x96b9df,
                blue_ink: 0x285a85,
                ice: 0x285a85,
                selected: 0xe8ddc7,
                good: 0x32623b,
                bad: 0x943e3e,
                on_fill: 0x191c1e,
            },
            (Skin::Lcars, Tone::Dark) => Self {
                lcars: true,
                dark: true,
                bg: 0x000000,
                panel: 0x0e0e0e,
                editor: 0x000000,
                raised: 0x1a1a1a,
                line: 0x333333,
                text: 0xffeecc,
                muted: 0x999999,
                accent: 0xff7000,
                accent_ink: 0xff7000,
                blue: 0x6699ff,
                blue_ink: 0x6699ff,
                ice: 0x99ccff,
                selected: 0x222244,
                good: 0x00cc00,
                bad: 0xcc6666,
                on_fill: 0x000000,
            },
            (Skin::Lcars, Tone::Light) => Self {
                lcars: true,
                dark: false,
                bg: 0xffeecc,
                panel: 0xfff7e7,
                editor: 0xfffdf7,
                raised: 0xf2e6cd,
                line: 0x9d8b70,
                text: 0x202020,
                muted: 0x665947,
                accent: 0xff7000,
                accent_ink: 0x9d4400,
                blue: 0x6699ff,
                blue_ink: 0x285aa8,
                ice: 0x285aa8,
                selected: 0xd6e5ff,
                good: 0x166016,
                bad: 0x993333,
                on_fill: 0x000000,
            },
        }
    }

    /// The tone of this theme.
    pub fn tone(&self) -> Tone {
        if self.dark {
            Tone::Dark
        } else {
            Tone::Light
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Appearance; 4] = [
        Appearance {
            skin: Skin::Standard,
            tone: Tone::Dark,
        },
        Appearance {
            skin: Skin::Standard,
            tone: Tone::Light,
        },
        Appearance {
            skin: Skin::Lcars,
            tone: Tone::Dark,
        },
        Appearance {
            skin: Skin::Lcars,
            tone: Tone::Light,
        },
    ];

    fn luminance(color: u32) -> f64 {
        let channel = |shift: u32| {
            let v = f64::from((color >> shift) & 0xff) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_is_readable_on_every_surface_of_every_appearance() {
        for appearance in ALL {
            let t = Theme::of(appearance);
            for surface in [t.bg, t.panel, t.editor, t.raised] {
                assert!(contrast(t.text, surface) >= 7.0, "{appearance:?} text");
                assert!(contrast(t.muted, surface) >= 4.5, "{appearance:?} muted");
            }
            assert!(
                contrast(t.accent_ink, t.panel) >= 4.5,
                "{appearance:?} accent ink"
            );
            assert!(
                contrast(t.blue_ink, t.panel) >= 4.5,
                "{appearance:?} blue ink"
            );
            assert!(
                contrast(t.on_fill, t.accent) >= 4.5,
                "{appearance:?} on accent"
            );
            assert!(contrast(t.on_fill, t.blue) >= 4.5, "{appearance:?} on blue");
            assert!(
                contrast(t.text, t.selected) >= 7.0,
                "{appearance:?} selected"
            );
        }
    }

    #[test]
    fn only_the_lcars_skin_is_lcars_and_the_tone_round_trips() {
        for appearance in ALL {
            let t = Theme::of(appearance);
            assert_eq!(t.lcars, appearance.skin == Skin::Lcars);
            assert_eq!(t.tone(), appearance.tone);
        }
    }
}
