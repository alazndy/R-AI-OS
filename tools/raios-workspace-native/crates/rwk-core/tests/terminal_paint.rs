use rwk_core::terminal::paint::build_paint_list;
use rwk_core::terminal::palette::{Palette, Rgb, Surface};
use rwk_core::terminal::{Cursor, CursorShape, Line, Modes, Run, Snapshot, Style, TermColor};

fn palette() -> Palette {
    Palette::dark(Surface {
        foreground: Rgb::from_hex(0xd4d4d4),
        background: Rgb::from_hex(0x1e1e1e),
        cursor: Rgb::from_hex(0xffffff),
        selection: Rgb::from_hex(0x264f78),
    })
}

fn run(col: u16, text: &str, style: Style) -> Run {
    Run {
        col,
        cells: text.chars().count() as u16,
        text: text.to_owned(),
        style,
    }
}

fn snapshot(lines: Vec<Line>) -> Snapshot {
    Snapshot {
        cols: 20,
        rows: lines.len() as u16,
        lines,
        cursor: None,
        modes: Modes::default(),
        display_offset: 0,
        history_size: 0,
        selection: Vec::new(),
        title: String::new(),
    }
}

#[test]
fn plain_text_paints_no_background() {
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "hello", Style::PLAIN)],
        }]),
        &palette(),
    );
    assert!(list.backgrounds.is_empty());
    assert_eq!(list.texts.len(), 1);
    assert_eq!(list.texts[0].text, "hello");
    assert_eq!(list.texts[0].color, Rgb::from_hex(0xd4d4d4));
}

#[test]
fn adjacent_runs_with_the_same_background_merge_into_one_rectangle() {
    let red = Style {
        bg: TermColor::Indexed(1),
        ..Style::PLAIN
    };
    let bold_red = Style { bold: true, ..red };
    let line = Line {
        runs: vec![run(0, "ab", red), run(2, "cd", bold_red)],
    };
    let list = build_paint_list(&snapshot(vec![line]), &palette());
    assert_eq!(list.backgrounds.len(), 1);
    assert_eq!((list.backgrounds[0].col, list.backgrounds[0].cells), (0, 4));
    assert_eq!(list.texts.len(), 2);
}

#[test]
fn inverse_swaps_resolved_colours() {
    let inverse = Style {
        inverse: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "x", inverse)],
        }]),
        &palette(),
    );
    assert_eq!(list.backgrounds[0].color, Rgb::from_hex(0xd4d4d4));
    assert_eq!(list.texts[0].color, Rgb::from_hex(0x1e1e1e));
}

#[test]
fn hidden_cells_paint_no_text() {
    let hidden = Style {
        hidden: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "secret", hidden)],
        }]),
        &palette(),
    );
    assert!(list.texts.is_empty());
}

#[test]
fn blank_runs_without_decoration_are_not_shaped() {
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "   ", Style::PLAIN)],
        }]),
        &palette(),
    );
    assert!(list.texts.is_empty());
    let underlined = Style {
        underline: true,
        ..Style::PLAIN
    };
    let list = build_paint_list(
        &snapshot(vec![Line {
            runs: vec![run(0, "   ", underlined)],
        }]),
        &palette(),
    );
    assert_eq!(list.texts.len(), 1);
}

#[test]
fn the_cursor_and_selection_are_carried_through() {
    let mut s = snapshot(vec![Line::default()]);
    s.cursor = Some(Cursor {
        col: 3,
        row: 0,
        shape: CursorShape::Beam,
    });
    let list = build_paint_list(&s, &palette());
    let cursor = list.cursor.expect("the cursor is visible");
    assert_eq!((cursor.row, cursor.col), (0, 3));
    assert_eq!(cursor.color, Rgb::from_hex(0xffffff));
}
