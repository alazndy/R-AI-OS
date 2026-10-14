//! Plain-data view of the visible terminal.
//!
//! A [`Snapshot`] holds no reference to the emulator. The UI takes one per frame, so the lock
//! on the emulator is held only while the visible rows are copied. Cells of identical style are
//! already merged into [`Run`]s, which is what a renderer shapes in one call.

/// A terminal colour before the theme resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermColor {
    /// The theme's default foreground or background.
    Default,
    /// An entry of the 256-colour palette: 0-15 are the ANSI colours, 16-255 the cube and greys.
    Indexed(u8),
    /// A 24-bit colour chosen by the program.
    Rgb(u8, u8, u8),
}

/// How a run of cells is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    /// Foreground colour.
    pub fg: TermColor,
    /// Background colour.
    pub bg: TermColor,
    /// Bold weight.
    pub bold: bool,
    /// Dimmed intensity.
    pub dim: bool,
    /// Italic slant.
    pub italic: bool,
    /// Any underline variant.
    pub underline: bool,
    /// Strike-through.
    pub strikeout: bool,
    /// Foreground and background are swapped by the renderer.
    pub inverse: bool,
    /// The text must be drawn as blanks.
    pub hidden: bool,
}

impl Style {
    /// The style of untouched cells.
    pub const PLAIN: Style = Style {
        fg: TermColor::Default,
        bg: TermColor::Default,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
        strikeout: false,
        inverse: false,
        hidden: false,
    };
}

/// Adjacent cells of one style on one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Column of the first cell.
    pub col: u16,
    /// Number of cells the run covers (wide characters count twice).
    pub cells: u16,
    /// The characters, including combining marks.
    pub text: String,
    /// Shared style.
    pub style: Style,
}

/// One visible row. Trailing blank cells with the plain style are not stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    /// Runs from left to right.
    pub runs: Vec<Run>,
}

/// Shape of the text cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    /// A filled cell.
    Block,
    /// A line under the cell.
    Underline,
    /// A vertical bar before the cell.
    Beam,
}

/// Position of the text cursor inside the visible area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Column, zero based.
    pub col: u16,
    /// Row, zero based.
    pub row: u16,
    /// Shape requested by the program.
    pub shape: CursorShape,
}

/// Input modes the program has switched on, which change how the UI encodes input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    /// Pasted text must be wrapped in bracketed-paste markers.
    pub bracketed_paste: bool,
    /// Arrow keys send application sequences.
    pub application_cursor: bool,
    /// The keypad sends application sequences.
    pub application_keypad: bool,
    /// The alternate screen is shown (no scrollback).
    pub alt_screen: bool,
    /// Mouse clicks are reported to the program.
    pub mouse_report: bool,
    /// Mouse drags are reported to the program.
    pub mouse_drag: bool,
    /// All mouse motion is reported to the program.
    pub mouse_motion: bool,
    /// Mouse reports use the SGR encoding.
    pub sgr_mouse: bool,
    /// Focus changes are reported to the program.
    pub focus_reporting: bool,
    /// The wheel is translated to arrow keys on the alternate screen.
    pub alternate_scroll: bool,
}

/// The selected cells of one visible row, columns inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowSpan {
    /// Visible row, zero based.
    pub row: u16,
    /// First selected column.
    pub start_col: u16,
    /// Last selected column.
    pub end_col: u16,
}

/// Everything a renderer needs for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Columns of the visible area.
    pub cols: u16,
    /// Rows of the visible area.
    pub rows: u16,
    /// One entry per visible row, top to bottom.
    pub lines: Vec<Line>,
    /// The cursor, or `None` while it is hidden or scrolled out of view.
    pub cursor: Option<Cursor>,
    /// Input modes.
    pub modes: Modes,
    /// Lines the view is scrolled back from the bottom; 0 follows the output.
    pub display_offset: usize,
    /// Lines of scrollback currently stored.
    pub history_size: usize,
    /// Selected cells inside the visible area.
    pub selection: Vec<RowSpan>,
    /// Window title requested by the program, sanitized.
    pub title: String,
}

impl Snapshot {
    /// The text of one visible row with trailing blanks removed, for tests and accessibility.
    pub fn row_text(&self, row: usize) -> String {
        let Some(line) = self.lines.get(row) else {
            return String::new();
        };
        let mut text = String::new();
        let mut col = 0usize;
        for run in &line.runs {
            while col < usize::from(run.col) {
                text.push(' ');
                col += 1;
            }
            text.push_str(&run.text);
            col = usize::from(run.col) + usize::from(run.cells);
        }
        text.trim_end().to_owned()
    }

    /// All visible rows joined by newlines, trailing blank rows removed.
    pub fn screen_text(&self) -> String {
        let mut rows: Vec<String> = (0..self.lines.len())
            .map(|row| self.row_text(row))
            .collect();
        while rows.last().is_some_and(|row| row.is_empty()) {
            rows.pop();
        }
        rows.join("\n")
    }
}
