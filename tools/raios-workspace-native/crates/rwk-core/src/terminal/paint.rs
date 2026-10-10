//! The renderer's decisions without a window: from a [`Snapshot`] and a [`Palette`] to plain
//! rectangles and text items. A toolkit paints them one-to-one.

use super::palette::{Palette, Rgb};
use super::snapshot::{CursorShape, RowSpan, Snapshot};

/// A filled background rectangle covering whole cells of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgRect {
    /// Visible row.
    pub row: u16,
    /// First column.
    pub col: u16,
    /// Width in cells.
    pub cells: u16,
    /// Fill colour.
    pub color: Rgb,
}

/// Text drawn from one column, with its decorations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextItem {
    /// Visible row.
    pub row: u16,
    /// Column of the first cell.
    pub col: u16,
    /// Cells covered.
    pub cells: u16,
    /// The characters.
    pub text: String,
    /// Foreground colour after inverse and dim.
    pub color: Rgb,
    /// Bold weight.
    pub bold: bool,
    /// Italic slant.
    pub italic: bool,
    /// Underline.
    pub underline: bool,
    /// Strike-through.
    pub strikeout: bool,
}

/// Where and how the cursor is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPaint {
    /// Visible row.
    pub row: u16,
    /// Column.
    pub col: u16,
    /// Shape requested by the program.
    pub shape: CursorShape,
    /// Cursor colour.
    pub color: Rgb,
}

/// Everything a toolkit paints for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintList {
    /// Columns of the grid.
    pub cols: u16,
    /// Rows of the grid.
    pub rows: u16,
    /// Background rectangles, merged along rows.
    pub backgrounds: Vec<BgRect>,
    /// Text items.
    pub texts: Vec<TextItem>,
    /// The cursor, when visible.
    pub cursor: Option<CursorPaint>,
    /// Selected cells (painted with [`Palette::selection`]).
    pub selection: Vec<RowSpan>,
}

/// Builds the paint list for one snapshot.
pub fn build_paint_list(snapshot: &Snapshot, palette: &Palette) -> PaintList {
    let mut backgrounds: Vec<BgRect> = Vec::new();
    let mut texts = Vec::new();
    for (row_index, line) in snapshot.lines.iter().enumerate() {
        let row = row_index as u16;
        for run in &line.runs {
            let style = run.style;
            let mut fg = palette.resolve(style.fg, palette.foreground);
            let mut bg = palette.resolve(style.bg, palette.background);
            if style.inverse {
                std::mem::swap(&mut fg, &mut bg);
            }
            if style.dim {
                fg = palette.dim(fg, bg);
            }
            if bg != palette.background {
                push_background(&mut backgrounds, row, run.col, run.cells, bg);
            }
            let blank = run.text.chars().all(char::is_whitespace);
            let decorated = style.underline || style.strikeout;
            if style.hidden || (blank && !decorated) {
                continue;
            }
            texts.push(TextItem {
                row,
                col: run.col,
                cells: run.cells,
                text: run.text.clone(),
                color: fg,
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
                strikeout: style.strikeout,
            });
        }
    }
    PaintList {
        cols: snapshot.cols,
        rows: snapshot.rows,
        backgrounds,
        texts,
        cursor: snapshot.cursor.map(|cursor| CursorPaint {
            row: cursor.row,
            col: cursor.col,
            shape: cursor.shape,
            color: palette.cursor,
        }),
        selection: snapshot.selection.clone(),
    }
}

fn push_background(list: &mut Vec<BgRect>, row: u16, col: u16, cells: u16, color: Rgb) {
    if let Some(last) = list.last_mut() {
        if last.row == row && last.color == color && last.col + last.cells == col {
            last.cells += cells;
            return;
        }
    }
    list.push(BgRect {
        row,
        col,
        cells,
        color,
    });
}
