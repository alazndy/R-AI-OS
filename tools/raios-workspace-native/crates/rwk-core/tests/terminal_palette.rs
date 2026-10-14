use rwk_core::terminal::palette::{Palette, Rgb, Surface};
use rwk_core::terminal::TermColor;

fn surface() -> Surface {
    Surface {
        foreground: Rgb::from_hex(0xd4d4d4),
        background: Rgb::from_hex(0x1e1e1e),
        cursor: Rgb::from_hex(0xffffff),
        selection: Rgb::from_hex(0x264f78),
    }
}

#[test]
fn indexed_colours_follow_the_xterm_cube_and_greys() {
    let palette = Palette::dark(surface());
    let default = Rgb(1, 2, 3);
    // 196 = 16 + 180: r=5 (255), g=0, b=0.
    assert_eq!(
        palette.resolve(TermColor::Indexed(196), default),
        Rgb(255, 0, 0)
    );
    // 16 is the cube origin, 231 its far corner.
    assert_eq!(
        palette.resolve(TermColor::Indexed(16), default),
        Rgb(0, 0, 0)
    );
    assert_eq!(
        palette.resolve(TermColor::Indexed(231), default),
        Rgb(255, 255, 255)
    );
    // Greys: 8 + 10 * (index - 232).
    assert_eq!(
        palette.resolve(TermColor::Indexed(232), default),
        Rgb(8, 8, 8)
    );
    assert_eq!(
        palette.resolve(TermColor::Indexed(255), default),
        Rgb(238, 238, 238)
    );
}

#[test]
fn the_sixteen_ansi_entries_come_from_the_workspace_0_1_palette() {
    let dark = Palette::dark(surface());
    let light = Palette::light(surface());
    let default = Rgb(0, 0, 0);
    assert_eq!(
        dark.resolve(TermColor::Indexed(1), default),
        Rgb::from_hex(0xcd3131)
    );
    assert_eq!(
        dark.resolve(TermColor::Indexed(15), default),
        Rgb::from_hex(0xffffff)
    );
    assert_eq!(
        light.resolve(TermColor::Indexed(1), default),
        Rgb::from_hex(0xa12622)
    );
    assert_eq!(
        light.resolve(TermColor::Indexed(15), default),
        Rgb::from_hex(0x7d8792)
    );
}

#[test]
fn the_default_colour_and_24_bit_colours_pass_through() {
    let palette = Palette::dark(surface());
    assert_eq!(
        palette.resolve(TermColor::Default, Rgb(9, 9, 9)),
        Rgb(9, 9, 9)
    );
    assert_eq!(
        palette.resolve(TermColor::Rgb(10, 20, 30), Rgb(9, 9, 9)),
        Rgb(10, 20, 30)
    );
}

#[test]
fn dim_moves_a_colour_a_third_of_the_way_to_the_background() {
    let palette = Palette::dark(surface());
    assert_eq!(
        palette.dim(Rgb(255, 255, 255), Rgb(0, 0, 0)),
        Rgb(170, 170, 170)
    );
}
