//! Shared visual identity for the terminal surface.
//!
//! The palette is sampled from `asciinator_6Aug_002.html`: a hot orange
//! perimeter around an electric-blue core on a near-black navy canvas.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub(crate) const BRAND_ORANGE: Color = Color::Rgb(255, 134, 0);
pub(crate) const BRAND_ORANGE_DEEP: Color = Color::Rgb(255, 99, 0);
pub(crate) const BRAND_BLUE: Color = Color::Rgb(0, 172, 255);
pub(crate) const BRAND_BLUE_DEEP: Color = Color::Rgb(0, 125, 226);
pub(crate) const BRAND_INK: Color = Color::Rgb(4, 8, 16);
pub(crate) const BRAND_PANEL: Color = Color::Rgb(7, 15, 27);

struct BrandLogoLine {
    orange_left: &'static str,
    blue_core: &'static str,
    orange_right: &'static str,
}

// Compact terminal adaptation of the supplied circular ASCII mark. Keeping
// the mark at six rows preserves the dashboard's existing responsive header.
const BRAND_LOGO: [BrandLogoLine; 6] = [
    BrandLogoLine {
        orange_left: "        .:--",
        blue_core: "========",
        orange_right: "--:.        ",
    },
    BrandLogoLine {
        orange_left: "    .:--'    ",
        blue_core: ".-====-.",
        orange_right: "    '--:.    ",
    },
    BrandLogoLine {
        orange_left: " .:--'       ",
        blue_core: "/ R-AI \\",
        orange_right: "       '--:. ",
    },
    BrandLogoLine {
        orange_left: " .:--.       ",
        blue_core: "\\  OS  /",
        orange_right: "       .--:. ",
    },
    BrandLogoLine {
        orange_left: "    ':--.    ",
        blue_core: "'-====-'",
        orange_right: "    .--:'    ",
    },
    BrandLogoLine {
        orange_left: "        ':--",
        blue_core: "========",
        orange_right: "--:'        ",
    },
];

/// Builds the two-tone ASCII mark used consistently by compact TUI surfaces.
pub(crate) fn brand_logo_lines() -> Vec<Line<'static>> {
    BRAND_LOGO
        .iter()
        .map(|line| {
            Line::from(vec![
                Span::styled(
                    line.orange_left,
                    Style::new().fg(BRAND_ORANGE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    line.blue_core,
                    Style::new().fg(BRAND_BLUE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    line.orange_right,
                    Style::new()
                        .fg(BRAND_ORANGE_DEEP)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_preserves_the_six_row_header_contract() {
        assert_eq!(brand_logo_lines().len(), 6);
        assert!(BRAND_LOGO.iter().all(|line| !line.blue_core.is_empty()));
    }
}
