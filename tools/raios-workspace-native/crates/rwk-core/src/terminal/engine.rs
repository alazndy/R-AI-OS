//! The terminal engine: bytes in, a drawable [`Snapshot`] out.
//!
//! Output: the PTY feeds a reader thread (64 KiB reads), which hands chunks over a bounded channel
//! to a parser thread. That thread owns the emulator behind a fair mutex; the UI calls
//! [`snapshot`](TerminalEngine::snapshot), which takes a short lock.
//!
//! Input: keys, pastes and query replies go through a bounded queue to a writer thread, which
//! writes to the PTY.
//!
//! * **Backpressure.** A full channel blocks the reader, the PTY fills and the producer slows
//!   down, so memory stays bounded however fast a program prints.
//! * **No blocking write on a hot thread.** Everything written to the PTY goes through a bounded
//!   queue to a writer thread. If the child stops reading its input, only that queue fills; the
//!   parser keeps draining output, so the child can never deadlock the engine.
//! * **The UI never parses.** The parser thread sets a dirty flag and calls the wake callback at
//!   most once until the UI has taken its next snapshot.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{viewport_to_point, Config, Osc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape as AnsiCursorShape, Processor, Rgb};

use super::paste::encode_paste;
use super::snapshot::{Cursor, CursorShape, Line, Modes, RowSpan, Run, Snapshot, Style, TermColor};
use crate::error::{AppError, ErrorCode, Result};
use crate::sessions::TerminalSize;

/// Smallest and largest terminal the engine accepts.
const MIN_COLS: u16 = 2;
const MAX_COLS: u16 = 500;
const MIN_ROWS: u16 = 2;
const MAX_ROWS: u16 = 300;
/// Largest single input packet, as in Workspace 0.1.
const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Longest window title kept.
const MAX_TITLE_CHARS: usize = 256;
/// Bytes parsed under one hold of the emulator lock.
const PARSE_SLICE_BYTES: usize = 8 * 1024;

/// Tunable engine parameters. The defaults are the product settings.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Initial terminal size.
    pub size: TerminalSize,
    /// Lines of scrollback kept.
    pub scrollback_lines: usize,
    /// Bytes requested per PTY read.
    pub read_chunk_bytes: usize,
    /// Chunks that may wait between the reader and the parser: the memory bound is
    /// `read_chunk_bytes * channel_capacity`.
    pub channel_capacity: usize,
    /// Messages that may wait for the PTY writer.
    pub input_capacity: usize,
    /// Default foreground reported to programs that ask (OSC 10), as `0xRRGGBB`.
    pub foreground: u32,
    /// Default background reported to programs that ask (OSC 11), as `0xRRGGBB`.
    pub background: u32,
    /// Size of one cell in pixels, reported to programs that ask.
    pub cell_pixels: (u16, u16),
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            size: TerminalSize {
                cols: 100,
                rows: 30,
            },
            scrollback_lines: 2_000,
            read_chunk_bytes: 64 * 1024,
            channel_capacity: 16,
            input_capacity: 256,
            foreground: 0x00d0_d0d0,
            background: 0x0014_1414,
            cell_pixels: (8, 16),
        }
    }
}

/// Counters for tests and the benchmark harness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// Bytes read from the PTY.
    pub bytes_read: u64,
    /// Bytes the parser has applied to the terminal.
    pub bytes_parsed: u64,
    /// Replies to terminal queries that were dropped because the input queue was full.
    pub dropped_replies: u64,
}

/// How the view moves through the scrollback. Positive line counts go toward older output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollRequest {
    /// Move by lines; positive is toward older output.
    Lines(i32),
    /// One screen toward older output.
    PageUp,
    /// One screen toward newer output.
    PageDown,
    /// The oldest stored line.
    Top,
    /// Follow the output again.
    Bottom,
}

/// What a selection covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Characters between two points.
    Simple,
    /// Whole words.
    Word,
    /// Whole lines.
    Line,
    /// A rectangle.
    Block,
}

/// Which half of a cell a mouse position fell on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellSide {
    /// The left half.
    Left,
    /// The right half.
    Right,
}

/// State shared with the emulator's event listener.
struct Meta {
    title: Mutex<String>,
    cols: AtomicU16,
    rows: AtomicU16,
    cell_width: AtomicU16,
    cell_height: AtomicU16,
    foreground: AtomicU32,
    background: AtomicU32,
    dropped_replies: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Receives the emulator's events. Only query replies and the title matter; clipboard events
/// never arrive because OSC 52 is disabled in the emulator configuration.
#[derive(Clone)]
struct Listener {
    replies: SyncSender<Vec<u8>>,
    meta: Arc<Meta>,
}

impl Listener {
    fn reply(&self, bytes: Vec<u8>) {
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.replies.try_send(bytes)
        {
            self.meta.dropped_replies.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn theme_color(&self, index: usize) -> Option<Rgb> {
        let packed = match index {
            256 | 258 => self.meta.foreground.load(Ordering::Relaxed),
            257 => self.meta.background.load(Ordering::Relaxed),
            _ => return None,
        };
        Some(Rgb {
            r: (packed >> 16) as u8,
            g: (packed >> 8) as u8,
            b: packed as u8,
        })
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.reply(text.into_bytes()),
            Event::ColorRequest(index, format) => {
                if let Some(rgb) = self.theme_color(index) {
                    self.reply(format(rgb).into_bytes());
                }
            }
            Event::TextAreaSizeRequest(format) => {
                let size = WindowSize {
                    num_lines: self.meta.rows.load(Ordering::Relaxed),
                    num_cols: self.meta.cols.load(Ordering::Relaxed),
                    cell_width: self.meta.cell_width.load(Ordering::Relaxed),
                    cell_height: self.meta.cell_height.load(Ordering::Relaxed),
                };
                self.reply(format(size).into_bytes());
            }
            Event::Title(title) => *lock(&self.meta.title) = sanitize_title(&title),
            Event::ResetTitle => lock(&self.meta.title).clear(),
            _ => {}
        }
    }
}

/// Removes control characters and bounds the length of a program-supplied title.
fn sanitize_title(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect()
}

struct Dims {
    cols: usize,
    rows: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

struct Shared {
    term: FairMutex<Term<Listener>>,
    meta: Arc<Meta>,
    bytes_read: AtomicU64,
    bytes_parsed: AtomicU64,
    dirty: AtomicBool,
    output_closed: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Shared {
    /// Marks new content and wakes the UI, but only once until the next snapshot.
    fn signal(&self) {
        if !self.dirty.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}

/// One terminal: emulator state plus the threads that feed it.
pub struct TerminalEngine {
    shared: Arc<Shared>,
    input: SyncSender<Vec<u8>>,
}

fn validate_size(size: TerminalSize) -> Result<()> {
    if !(MIN_COLS..=MAX_COLS).contains(&size.cols) || !(MIN_ROWS..=MAX_ROWS).contains(&size.rows) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Invalid terminal dimensions.",
        ));
    }
    Ok(())
}

/// Makes the calling thread slightly less eager than the UI thread. During a flood the reader
/// and parser threads of several sessions can use every core and starve the thread that paints;
/// a few nice levels keep the flood fast while the window stays responsive. Best effort: a
/// failure only means the thread keeps its default priority.
#[cfg(target_os = "linux")]
fn lower_thread_priority() {
    const BACKGROUND_NICE: libc::c_int = 6;
    // SAFETY: `setpriority` with `PRIO_PROCESS` and id 0 touches only the calling thread's
    // scheduling priority (a thread is a task on Linux) and reads no memory of ours.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, BACKGROUND_NICE);
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_thread_priority() {}

fn spawn_thread(name: &str, body: impl FnOnce() + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            lower_thread_priority();
            body();
        })
        .map(|_| ())
        .map_err(|error| {
            AppError::new(ErrorCode::Internal, format!("cannot start {name}: {error}"))
        })
}

impl TerminalEngine {
    /// Starts an engine on a byte stream.
    ///
    /// `input` is what the program prints (the PTY master reader), `output` is where keys and
    /// query replies go (the PTY master writer). `wake` is called, from the parser thread, when
    /// new content is ready; it must be cheap and must not call back into the engine.
    ///
    /// The threads end when `input` reports end of file or an error. A caller that needs to
    /// cancel a blocked read supplies a reader that can be cancelled.
    pub fn start(
        input: Box<dyn Read + Send>,
        output: Box<dyn Write + Send>,
        options: EngineOptions,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        validate_size(options.size)?;
        let (input_tx, input_rx) = sync_channel::<Vec<u8>>(options.input_capacity.max(1));
        let meta = Arc::new(Meta {
            title: Mutex::new(String::new()),
            cols: AtomicU16::new(options.size.cols),
            rows: AtomicU16::new(options.size.rows),
            cell_width: AtomicU16::new(options.cell_pixels.0),
            cell_height: AtomicU16::new(options.cell_pixels.1),
            foreground: AtomicU32::new(options.foreground),
            background: AtomicU32::new(options.background),
            dropped_replies: AtomicU64::new(0),
        });
        let config = Config {
            scrolling_history: options.scrollback_lines,
            // A program must not be able to read or write the user's clipboard.
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let dims = Dims {
            cols: usize::from(options.size.cols),
            rows: usize::from(options.size.rows),
        };
        let listener = Listener {
            replies: input_tx.clone(),
            meta: meta.clone(),
        };
        let shared = Arc::new(Shared {
            term: FairMutex::new(Term::new(config, &dims, listener)),
            meta,
            bytes_read: AtomicU64::new(0),
            bytes_parsed: AtomicU64::new(0),
            dirty: AtomicBool::new(false),
            output_closed: AtomicBool::new(false),
            wake,
        });
        let (chunk_tx, chunk_rx) = sync_channel::<Vec<u8>>(options.channel_capacity.max(1));
        let chunk_bytes = options.read_chunk_bytes.max(1);

        let reader_shared = shared.clone();
        spawn_thread("rwk-term-reader", move || {
            read_loop(input, chunk_tx, &reader_shared, chunk_bytes)
        })?;
        let parser_shared = shared.clone();
        spawn_thread("rwk-term-parser", move || {
            parse_loop(chunk_rx, &parser_shared)
        })?;
        spawn_thread("rwk-term-writer", move || write_loop(input_rx, output))?;
        Ok(Self {
            shared,
            input: input_tx,
        })
    }

    /// Copies the visible area. Holds the emulator lock only for the copy.
    pub fn snapshot(&self) -> Snapshot {
        // Cleared before locking, so output that arrives during the copy re-arms the wake-up.
        self.shared.dirty.store(false, Ordering::Release);
        let term = self.shared.term.lock();
        let title = lock(&self.shared.meta.title).clone();
        build_snapshot(&term, title)
    }

    /// Queues bytes the user typed. Fails instead of blocking when the program is not reading.
    pub fn send_input(&self, data: &[u8]) -> Result<()> {
        if data.len() > MAX_INPUT_BYTES {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Terminal input packet is too large.",
            ));
        }
        match self.input.try_send(data.to_vec()) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(AppError::new(
                ErrorCode::Conflict,
                "The program is not reading its input.",
            )),
            Err(TrySendError::Disconnected(_)) => Err(AppError::new(
                ErrorCode::NotFound,
                "This terminal session has ended.",
            )),
        }
    }

    /// Queues pasted text, wrapped in bracketed-paste markers when the program asked for them.
    pub fn paste(&self, text: &str) -> Result<()> {
        let bracketed = self
            .shared
            .term
            .lock()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        self.send_input(&encode_paste(text, bracketed)?)
    }

    /// Resizes the emulator. The caller resizes the PTY itself.
    pub fn resize(&self, size: TerminalSize) -> Result<()> {
        validate_size(size)?;
        let meta = &self.shared.meta;
        meta.cols.store(size.cols, Ordering::Relaxed);
        meta.rows.store(size.rows, Ordering::Relaxed);
        self.shared.term.lock().resize(Dims {
            cols: usize::from(size.cols),
            rows: usize::from(size.rows),
        });
        self.shared.signal();
        Ok(())
    }

    /// Scrolls the view through the scrollback.
    pub fn scroll(&self, request: ScrollRequest) {
        let scroll = match request {
            ScrollRequest::Lines(lines) => Scroll::Delta(lines),
            ScrollRequest::PageUp => Scroll::PageUp,
            ScrollRequest::PageDown => Scroll::PageDown,
            ScrollRequest::Top => Scroll::Top,
            ScrollRequest::Bottom => Scroll::Bottom,
        };
        self.shared.term.lock().scroll_display(scroll);
        self.shared.signal();
    }

    /// Starts a selection at a visible cell.
    pub fn start_selection(&self, kind: SelectionKind, row: u16, col: u16, side: CellSide) {
        let mut term = self.shared.term.lock();
        let point = viewport_to_point(
            term.grid().display_offset(),
            Point::new(usize::from(row), Column(usize::from(col))),
        );
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        term.selection = Some(Selection::new(ty, point, to_side(side)));
        drop(term);
        self.shared.signal();
    }

    /// Extends the selection to a visible cell.
    pub fn update_selection(&self, row: u16, col: u16, side: CellSide) {
        let mut term = self.shared.term.lock();
        let point = viewport_to_point(
            term.grid().display_offset(),
            Point::new(usize::from(row), Column(usize::from(col))),
        );
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, to_side(side));
        }
        drop(term);
        self.shared.signal();
    }

    /// Removes the selection.
    pub fn clear_selection(&self) {
        self.shared.term.lock().selection = None;
        self.shared.signal();
    }

    /// The selected text, if there is a selection.
    pub fn selection_text(&self) -> Option<String> {
        self.shared.term.lock().selection_to_string()
    }

    /// Updates the colours reported to programs that ask for the default colours.
    pub fn set_theme_colors(&self, foreground: u32, background: u32) {
        let meta = &self.shared.meta;
        meta.foreground.store(foreground, Ordering::Relaxed);
        meta.background.store(background, Ordering::Relaxed);
    }

    /// Updates the cell size reported to programs that ask for the text area size in pixels.
    pub fn set_cell_pixels(&self, width: u16, height: u16) {
        self.shared.meta.cell_width.store(width, Ordering::Relaxed);
        self.shared
            .meta
            .cell_height
            .store(height, Ordering::Relaxed);
    }

    /// Current counters.
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            bytes_read: self.shared.bytes_read.load(Ordering::Acquire),
            bytes_parsed: self.shared.bytes_parsed.load(Ordering::Acquire),
            dropped_replies: self.shared.meta.dropped_replies.load(Ordering::Relaxed),
        }
    }

    /// True once the input ended and everything read has been parsed.
    pub fn is_output_closed(&self) -> bool {
        self.shared.output_closed.load(Ordering::Acquire)
    }

    /// Waits until [`is_output_closed`](Self::is_output_closed) or the timeout; returns whether
    /// the output closed.
    pub fn wait_output_closed(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.is_output_closed() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
        true
    }
}

fn to_side(side: CellSide) -> Side {
    match side {
        CellSide::Left => Side::Left,
        CellSide::Right => Side::Right,
    }
}

fn read_loop(
    mut input: Box<dyn Read + Send>,
    chunks: SyncSender<Vec<u8>>,
    shared: &Shared,
    chunk_bytes: usize,
) {
    let mut buffer = vec![0u8; chunk_bytes];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                shared.bytes_read.fetch_add(count as u64, Ordering::AcqRel);
                if chunks.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            // A PTY master reports EIO once the last slave descriptor closed: end of output.
            Err(_) => break,
        }
    }
}

fn parse_loop(chunks: Receiver<Vec<u8>>, shared: &Shared) {
    let mut processor: Processor = Processor::new();
    while let Ok(chunk) = chunks.recv() {
        // The lock is released between slices, so a UI waiting for its snapshot never waits for
        // more than one slice, however large the chunk the reader delivered.
        for slice in chunk.chunks(PARSE_SLICE_BYTES) {
            let mut term = shared.term.lock();
            processor.advance(&mut *term, slice);
        }
        shared
            .bytes_parsed
            .fetch_add(chunk.len() as u64, Ordering::AcqRel);
        shared.signal();
    }
    shared.output_closed.store(true, Ordering::Release);
    shared.signal();
}

fn write_loop(messages: Receiver<Vec<u8>>, mut output: Box<dyn Write + Send>) {
    while let Ok(bytes) = messages.recv() {
        if output
            .write_all(&bytes)
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
    }
}

fn convert_color(color: Color) -> TermColor {
    match color {
        Color::Spec(Rgb { r, g, b }) => TermColor::Rgb(r, g, b),
        Color::Indexed(index) => TermColor::Indexed(index),
        Color::Named(named) => match named as usize {
            index @ 0..=15 => TermColor::Indexed(index as u8),
            index @ 259..=266 => TermColor::Indexed((index - 259) as u8),
            _ => TermColor::Default,
        },
    }
}

fn is_dim_named(color: Color) -> bool {
    matches!(color, Color::Named(named) if (259..=266).contains(&(named as usize)))
}

fn cell_style(cell: &alacritty_terminal::term::cell::Cell) -> Style {
    let flags = cell.flags;
    Style {
        fg: convert_color(cell.fg),
        bg: convert_color(cell.bg),
        bold: flags.contains(Flags::BOLD),
        dim: flags.contains(Flags::DIM) || is_dim_named(cell.fg),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strikeout: flags.contains(Flags::STRIKEOUT),
        inverse: flags.contains(Flags::INVERSE),
        hidden: flags.contains(Flags::HIDDEN),
    }
}

/// Removes the blanks at the end of a row that carry no style.
fn trim_trailing_blanks(runs: &mut Vec<Run>) {
    while let Some(last) = runs.last_mut() {
        if last.style != Style::PLAIN {
            break;
        }
        let kept = last.text.trim_end_matches(' ').len();
        if kept == 0 {
            runs.pop();
            continue;
        }
        let removed = last.text.len() - kept;
        last.text.truncate(kept);
        last.cells -= removed as u16;
        break;
    }
}

fn build_snapshot(term: &Term<Listener>, title: String) -> Snapshot {
    let content = term.renderable_content();
    let display_offset = content.display_offset;
    let cols = term.columns();
    let rows = term.screen_lines();
    let mut lines: Vec<Line> = vec![Line::default(); rows];

    for indexed in content.display_iter {
        let row = indexed.point.line.0 + display_offset as i32;
        let Ok(row) = usize::try_from(row) else {
            continue;
        };
        let Some(line) = lines.get_mut(row) else {
            continue;
        };
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        let col = indexed.point.column.0 as u16;
        let width: u16 = if cell.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let style = cell_style(cell);
        match line.runs.last_mut() {
            Some(run) if run.style == style && run.col + run.cells == col => {
                run.text.push(cell.c);
                run.cells += width;
                if let Some(marks) = cell.zerowidth() {
                    run.text.extend(marks);
                }
            }
            _ => {
                let mut text = String::with_capacity(16);
                text.push(cell.c);
                if let Some(marks) = cell.zerowidth() {
                    text.extend(marks);
                }
                line.runs.push(Run {
                    col,
                    cells: width,
                    text,
                    style,
                });
            }
        }
    }
    for line in &mut lines {
        trim_trailing_blanks(&mut line.runs);
    }

    let cursor = if matches!(content.cursor.shape, AnsiCursorShape::Hidden)
        || !term.mode().contains(TermMode::SHOW_CURSOR)
    {
        None
    } else {
        let row = content.cursor.point.line.0 + display_offset as i32;
        usize::try_from(row)
            .ok()
            .filter(|row| *row < rows)
            .map(|row| Cursor {
                col: content.cursor.point.column.0 as u16,
                row: row as u16,
                shape: match content.cursor.shape {
                    AnsiCursorShape::Underline => CursorShape::Underline,
                    AnsiCursorShape::Beam => CursorShape::Beam,
                    _ => CursorShape::Block,
                },
            })
    };

    let mode = *term.mode();
    let selection = content
        .selection
        .map(|range| {
            (0..rows)
                .filter_map(|row| {
                    let line = row as i32 - display_offset as i32;
                    if line < range.start.line.0 || line > range.end.line.0 {
                        return None;
                    }
                    let (start_col, end_col) = if range.is_block {
                        (
                            range.start.column.0.min(range.end.column.0),
                            range.start.column.0.max(range.end.column.0),
                        )
                    } else {
                        (
                            if line == range.start.line.0 {
                                range.start.column.0
                            } else {
                                0
                            },
                            if line == range.end.line.0 {
                                range.end.column.0
                            } else {
                                cols.saturating_sub(1)
                            },
                        )
                    };
                    Some(RowSpan {
                        row: row as u16,
                        start_col: start_col as u16,
                        end_col: end_col as u16,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Snapshot {
        cols: cols as u16,
        rows: rows as u16,
        lines,
        cursor,
        modes: Modes {
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            application_cursor: mode.contains(TermMode::APP_CURSOR),
            application_keypad: mode.contains(TermMode::APP_KEYPAD),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            mouse_report: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
            mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
        },
        display_offset,
        history_size: term.grid().history_size(),
        selection,
        title,
    }
}
