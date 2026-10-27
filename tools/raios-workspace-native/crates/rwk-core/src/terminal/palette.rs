//! Terminal colours: the 16 ANSI entries of Workspace 0.1, the xterm 6x6x6 cube and the grey
//! ramp. Surface colours (foreground, background, cursor, selection) follow the skin; the ANSI
//! entries stay a terminal palette, as in 0.1's `terminal-rendering.ts`.

use super::snapshot::TermColor;

/// An opaque 24-bit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Builds a colour from `0xRRGGBB`.
    pub const fn from_hex(value: u32) -> Rgb {
        Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }
}

/// The skin-dependent colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    /// Default text colour.
    pub foreground: Rgb,
    /// Default background colour.
    pub background: Rgb,
    /// Cursor colour.
    pub cursor: Rgb,
    /// Selection highlight colour.
    pub selection: Rgb,
}

const DARK_ANSI: [Rgb; 16] = [
    Rgb::from_hex(0x1e1e1e),
    Rgb::from_hex(0xcd3131),
    Rgb::from_hex(0x0dbc79),
    Rgb::from_hex(0xe5e510),
    Rgb::from_hex(0x2472c8),
    Rgb::from_hex(0xbc3fbc),
    Rgb::from_hex(0x11a8cd),
    Rgb::from_hex(0xe5e5e5),
    Rgb::from_hex(0x666666),
    Rgb::from_hex(0xf14c4c),
    Rgb::from_hex(0x23d18b),
    Rgb::from_hex(0xf5f543),
    Rgb::from_hex(0x3b8eea),
    Rgb::from_hex(0xd670d6),
    Rgb::from_hex(0x29b8db),
    Rgb::from_hex(0xffffff),
];

/// The Aperture terminal colours (the Konsole scheme of the KDE theme).
const APERTURE_ANSI: [Rgb; 16] = [
    Rgb::from_hex(0x141414),
    Rgb::from_hex(0xff4d4d),
    Rgb::from_hex(0x3ddc84),
    Rgb::from_hex(0xff9a00),
    Rgb::from_hex(0x00aeef),
    Rgb::from_hex(0xc58aff),
    Rgb::from_hex(0x00d2c8),
    Rgb::from_hex(0xc8c8c8),
    Rgb::from_hex(0x555555),
    Rgb::from_hex(0xff7878),
    Rgb::from_hex(0x78f0aa),
    Rgb::from_hex(0xffbf5e),
    Rgb::from_hex(0x5eceff),
    Rgb::from_hex(0xdcb4ff),
    Rgb::from_hex(0x64ebe4),
    Rgb::from_hex(0xffffff),
];

const LIGHT_ANSI: [Rgb; 16] = [
    Rgb::from_hex(0x141a23),
    Rgb::from_hex(0xa12622),
    Rgb::from_hex(0x276338),
    Rgb::from_hex(0x80651a),
    Rgb::from_hex(0x245da8),
    Rgb::from_hex(0x853f8f),
    Rgb::from_hex(0x216a75),
    Rgb::from_hex(0x49515a),
    Rgb::from_hex(0x69727c),
    Rgb::from_hex(0xcf3631),
    Rgb::from_hex(0x31824b),
    Rgb::from_hex(0x9c7a1f),
    Rgb::from_hex(0x3478ce),
    Rgb::from_hex(0xaa55b8),
    Rgb::from_hex(0x278593),
    Rgb::from_hex(0x7d8792),
];

/// Levels of one channel of the 6x6x6 colour cube.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// A complete terminal palette for one tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// ANSI colours 0-15.
    pub ansi: [Rgb; 16],
    /// Default text colour.
    pub foreground: Rgb,
    /// Default background colour.
    pub background: Rgb,
    /// Cursor colour.
    pub cursor: Rgb,
    /// Selection highlight colour.
    pub selection: Rgb,
}

impl Palette {
    /// The dark-tone palette on the given surface.
    pub const fn dark(surface: Surface) -> Palette {
        Palette::with_ansi(DARK_ANSI, surface)
    }

    /// The Aperture palette on the given surface.
    pub const fn aperture(surface: Surface) -> Palette {
        Palette::with_ansi(APERTURE_ANSI, surface)
    }

    /// The light-tone palette on the given surface.
    pub const fn light(surface: Surface) -> Palette {
        Palette::with_ansi(LIGHT_ANSI, surface)
    }

    const fn with_ansi(ansi: [Rgb; 16], surface: Surface) -> Palette {
        Palette {
            ansi,
            foreground: surface.foreground,
            background: surface.background,
            cursor: surface.cursor,
            selection: surface.selection,
        }
    }

    /// Resolves a terminal colour; `default` is used for [`TermColor::Default`].
    pub fn resolve(&self, color: TermColor, default: Rgb) -> Rgb {
        match color {
            TermColor::Default => default,
            TermColor::Rgb(r, g, b) => Rgb(r, g, b),
            TermColor::Indexed(index) if index < 16 => self.ansi[usize::from(index)],
            TermColor::Indexed(index) if index < 232 => {
                let offset = usize::from(index - 16);
                Rgb(
                    CUBE_LEVELS[offset / 36],
                    CUBE_LEVELS[(offset / 6) % 6],
                    CUBE_LEVELS[offset % 6],
                )
            }
            TermColor::Indexed(index) => {
                let grey = 8 + 10 * (index - 232);
                Rgb(grey, grey, grey)
            }
        }
    }

    /// The colour of dimmed text: a third of the way from `fg` to `bg`.
    pub fn dim(&self, fg: Rgb, bg: Rgb) -> Rgb {
        let mix = |a: u8, b: u8| ((u16::from(a) * 2 + u16::from(b)) / 3) as u8;
        Rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2))
    }
}
