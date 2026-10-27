//! The terminal engine over an in-memory pipe pair: no PTY, no UI.

use std::io::{PipeWriter, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rwk_core::error::ErrorCode;
use rwk_core::sessions::TerminalSize;
use rwk_core::terminal::{
    CellSide, CursorShape, EngineOptions, ScrollRequest, SelectionKind, Style, TermColor,
    TerminalEngine,
};

const WAIT: Duration = Duration::from_secs(10);

/// Collects everything the engine writes to the "PTY".
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

struct Rig {
    engine: TerminalEngine,
    output: Option<PipeWriter>,
    sink: Sink,
    wakes: Arc<AtomicUsize>,
}

fn size(cols: u16, rows: u16) -> TerminalSize {
    TerminalSize { cols, rows }
}

fn rig_with(options: EngineOptions, sink: Box<dyn Write + Send>, collected: Sink) -> Rig {
    let (reader, output) = std::io::pipe().unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    let engine = TerminalEngine::start(
        Box::new(reader),
        sink,
        options,
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    Rig {
        engine,
        output: Some(output),
        sink: collected,
        wakes,
    }
}

fn rig(cols: u16, rows: u16) -> Rig {
    let sink = Sink::default();
    rig_with(
        EngineOptions {
            size: size(cols, rows),
            ..EngineOptions::default()
        },
        Box::new(sink.clone()),
        sink,
    )
}

/// Sends `bytes` as program output, ends the output and waits until it is parsed.
fn print_and_finish(mut rig: Rig, bytes: &[u8]) -> Rig {
    let mut output = rig.output.take().expect("the output is still open");
    output.write_all(bytes).unwrap();
    drop(output);
    assert!(
        rig.engine.wait_output_closed(WAIT),
        "output was not parsed in time"
    );
    rig
}

/// The still open output of a rig, for tests that write in several steps.
fn output(rig: &mut Rig) -> &mut PipeWriter {
    rig.output.as_mut().expect("the output is still open")
}

fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn printed_text_and_line_breaks_appear_in_the_snapshot() {
    let rig = print_and_finish(rig(20, 5), b"hello\r\nworld");
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.row_text(0), "hello");
    assert_eq!(snapshot.row_text(1), "world");
    assert_eq!(snapshot.screen_text(), "hello\nworld");
    assert_eq!((snapshot.cols, snapshot.rows), (20, 5));
    let cursor = snapshot.cursor.unwrap();
    assert_eq!(
        (cursor.col, cursor.row, cursor.shape),
        (5, 1, CursorShape::Block)
    );
}

#[test]
fn adjacent_cells_of_one_style_are_merged_into_one_run() {
    let rig = print_and_finish(
        rig(40, 3),
        b"\x1b[31mred\x1b[0m plain \x1b[38;2;1;2;3mtrue\x1b[0m",
    );
    let line = &rig.engine.snapshot().lines[0];
    let summary: Vec<(u16, &str, TermColor)> = line
        .runs
        .iter()
        .map(|run| (run.col, run.text.as_str(), run.style.fg))
        .collect();
    assert_eq!(
        summary,
        vec![
            (0, "red", TermColor::Indexed(1)),
            (3, " plain ", TermColor::Default),
            (10, "true", TermColor::Rgb(1, 2, 3)),
        ]
    );
}

#[test]
fn trailing_plain_blanks_are_not_stored_but_coloured_blanks_are() {
    let rig = print_and_finish(rig(20, 3), b"abc   \r\n\x1b[44m   \x1b[0m");
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.lines[0].runs.len(), 1);
    assert_eq!(snapshot.lines[0].runs[0].text, "abc");
    assert_eq!(snapshot.lines[0].runs[0].cells, 3);
    assert_eq!(
        snapshot.lines[1].runs.len(),
        1,
        "a blue background must survive"
    );
    assert_eq!(snapshot.lines[1].runs[0].style.bg, TermColor::Indexed(4));
}

#[test]
fn wide_and_combining_characters_keep_their_cell_widths() {
    let rig = print_and_finish(rig(20, 3), "日本語 e\u{301}".as_bytes());
    let line = &rig.engine.snapshot().lines[0];
    assert_eq!(line.runs.len(), 1);
    assert_eq!(line.runs[0].text, "日本語 e\u{301}");
    assert_eq!(
        line.runs[0].cells, 8,
        "three wide characters, a space and one narrow cell"
    );
}

#[test]
fn bold_underline_and_inverse_are_reported_as_flags() {
    let rig = print_and_finish(rig(20, 3), b"\x1b[1;4;7mX\x1b[0m");
    let style = rig.engine.snapshot().lines[0].runs[0].style;
    assert!(style.bold && style.underline && style.inverse);
    assert!(!style.dim && !style.italic && !style.strikeout && !style.hidden);
    assert_ne!(style, Style::PLAIN);
}

#[test]
fn a_cursor_position_query_is_answered_on_the_pty() {
    let rig = print_and_finish(rig(20, 5), b"abc\x1b[6n");
    wait_for("the cursor position report", || {
        !rig.sink.bytes().is_empty()
    });
    assert_eq!(rig.sink.bytes(), b"\x1b[1;4R");
}

#[test]
fn a_device_attributes_query_is_answered_on_the_pty() {
    let rig = print_and_finish(rig(20, 5), b"\x1b[c");
    wait_for("the device attributes", || !rig.sink.bytes().is_empty());
    assert!(
        rig.sink.bytes().starts_with(b"\x1b[?"),
        "{:?}",
        rig.sink.bytes()
    );
}

#[test]
fn a_default_colour_query_reports_the_theme_colours() {
    let rig = rig(20, 5);
    rig.engine.set_theme_colors(0x00d0_d0d0, 0x0012_3456);
    let rig = print_and_finish(rig, b"\x1b]11;?\x07");
    wait_for("the background report", || !rig.sink.bytes().is_empty());
    let reply = String::from_utf8(rig.sink.bytes()).unwrap();
    assert!(reply.starts_with("\x1b]11;rgb:"), "{reply:?}");
    assert!(reply.contains("1212/3434/5656"), "{reply:?}");
}

#[test]
fn a_program_cannot_read_or_write_the_clipboard_through_osc_52() {
    let rig = print_and_finish(rig(20, 5), b"\x1b]52;c;?\x07\x1b]52;c;aGVsbG8=\x07ok");
    thread::sleep(Duration::from_millis(100));
    assert!(rig.sink.bytes().is_empty(), "no reply may be written");
    assert_eq!(rig.engine.snapshot().row_text(0), "ok");
}

#[test]
fn the_window_title_is_kept_without_control_characters_and_bounded() {
    let long = "t".repeat(1_000);
    let first = print_and_finish(rig(20, 5), format!("\x1b]0;{long}\x07").as_bytes());
    assert_eq!(first.engine.snapshot().title.len(), 256);
    let rig = print_and_finish(rig(20, 5), b"\x1b]2;my title\x07");
    assert_eq!(rig.engine.snapshot().title, "my title");
}

#[test]
fn modes_the_program_switches_on_are_reported() {
    let rig = print_and_finish(
        rig(20, 5),
        b"\x1b[?2004h\x1b[?1h\x1b[?1049h\x1b[?1000h\x1b[?1006h",
    );
    let modes = rig.engine.snapshot().modes;
    assert!(modes.bracketed_paste && modes.application_cursor && modes.alt_screen);
    assert!(modes.mouse_report && modes.sgr_mouse);
    assert!(!modes.mouse_motion);
}

#[test]
fn pasted_text_is_wrapped_only_when_the_program_asked_and_never_carries_escapes() {
    let plain = print_and_finish(rig(20, 5), b"");
    plain
        .engine
        .paste("line one\nline two\x1b[201~rm -rf /")
        .unwrap();
    wait_for("the plain paste", || !plain.sink.bytes().is_empty());
    assert_eq!(plain.sink.bytes(), b"line one\rline two[201~rm -rf /");

    let bracketed = print_and_finish(rig(20, 5), b"\x1b[?2004h");
    bracketed.engine.paste("a\nb\x1b[201~c").unwrap();
    wait_for("the bracketed paste", || !bracketed.sink.bytes().is_empty());
    assert_eq!(bracketed.sink.bytes(), b"\x1b[200~a\nb[201~c\x1b[201~");
}

#[test]
fn an_oversized_paste_or_input_packet_is_refused() {
    let rig = rig(20, 5);
    let paste = rig.engine.paste(&"x".repeat(16 * 1024 + 1)).unwrap_err();
    let input = rig
        .engine
        .send_input(&vec![b'x'; 64 * 1024 + 1])
        .unwrap_err();
    assert_eq!(paste.code, ErrorCode::InvalidInput);
    assert_eq!(input.code, ErrorCode::InvalidInput);
    assert!(rig.sink.bytes().is_empty());
}

#[test]
fn scrollback_keeps_old_lines_and_the_view_can_move_through_them() {
    let mut text = String::new();
    for line in 1..=100 {
        text.push_str(&format!("line {line}\r\n"));
    }
    let rig = print_and_finish(rig(20, 10), text.as_bytes());
    let bottom = rig.engine.snapshot();
    assert_eq!(bottom.display_offset, 0);
    assert!(bottom.history_size >= 90, "history {}", bottom.history_size);
    assert_eq!(bottom.row_text(8), "line 100");

    rig.engine.scroll(ScrollRequest::Lines(5));
    let scrolled = rig.engine.snapshot();
    assert_eq!(scrolled.display_offset, 5);
    assert_eq!(scrolled.row_text(8), "line 95");
    assert!(
        scrolled.cursor.is_none(),
        "the cursor is below the visible area"
    );

    rig.engine.scroll(ScrollRequest::Top);
    assert_eq!(rig.engine.snapshot().row_text(0), "line 1");
    rig.engine.scroll(ScrollRequest::Bottom);
    assert_eq!(rig.engine.snapshot().display_offset, 0);
}

#[test]
fn scrollback_is_bounded_by_the_option() {
    let sink = Sink::default();
    let rig = rig_with(
        EngineOptions {
            size: size(20, 5),
            scrollback_lines: 50,
            ..EngineOptions::default()
        },
        Box::new(sink.clone()),
        sink,
    );
    let mut text = String::new();
    for line in 1..=500 {
        text.push_str(&format!("line {line}\r\n"));
    }
    let rig = print_and_finish(rig, text.as_bytes());
    assert_eq!(rig.engine.snapshot().history_size, 50);
}

#[test]
fn resizing_changes_the_snapshot_and_rejects_impossible_sizes() {
    let rig = rig(20, 5);
    rig.engine.resize(size(40, 12)).unwrap();
    let snapshot = rig.engine.snapshot();
    assert_eq!(
        (snapshot.cols, snapshot.rows, snapshot.lines.len()),
        (40, 12, 12)
    );
    for bad in [size(1, 5), size(20, 1), size(501, 5), size(20, 301)] {
        assert_eq!(
            rig.engine.resize(bad).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(rig.engine.snapshot().cols, 40);
}

#[test]
fn a_selection_returns_the_selected_text() {
    let rig = print_and_finish(rig(30, 5), b"hello world\r\nsecond line");
    rig.engine
        .start_selection(SelectionKind::Simple, 0, 0, CellSide::Left);
    rig.engine.update_selection(0, 4, CellSide::Right);
    assert_eq!(rig.engine.selection_text().as_deref(), Some("hello"));
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.selection.len(), 1);
    assert_eq!(
        (
            snapshot.selection[0].row,
            snapshot.selection[0].start_col,
            snapshot.selection[0].end_col
        ),
        (0, 0, 4)
    );
    rig.engine.clear_selection();
    assert_eq!(rig.engine.selection_text(), None);
    assert!(rig.engine.snapshot().selection.is_empty());
}

#[test]
fn the_wake_up_fires_once_until_the_next_snapshot() {
    let mut rig = rig(20, 5);
    output(&mut rig).write_all(b"a").unwrap();
    wait_for("the first wake-up", || {
        rig.wakes.load(Ordering::SeqCst) == 1
    });
    output(&mut rig).write_all(b"b").unwrap();
    wait_for("the second byte", || rig.engine.stats().bytes_parsed == 2);
    assert_eq!(
        rig.wakes.load(Ordering::SeqCst),
        1,
        "no second wake-up before a snapshot"
    );

    let _ = rig.engine.snapshot();
    output(&mut rig).write_all(b"c").unwrap();
    wait_for("the wake-up after the snapshot", || {
        rig.wakes.load(Ordering::SeqCst) == 2
    });
}

#[test]
fn a_blocked_parser_bounds_how_much_is_read_ahead() {
    // The wake callback runs on the parser thread. Blocking it freezes the parser, which is the
    // situation the bounded channel exists for.
    let release = Arc::new(Barrier::new(2));
    let gate = release.clone();
    let first = Arc::new(AtomicUsize::new(0));
    let once = first.clone();
    let (reader, mut output) = std::io::pipe().unwrap();
    let sink = Sink::default();
    let options = EngineOptions {
        size: size(20, 5),
        read_chunk_bytes: 1024,
        channel_capacity: 2,
        ..EngineOptions::default()
    };
    let engine = TerminalEngine::start(
        Box::new(reader),
        Box::new(sink),
        options,
        Arc::new(move || {
            if once.fetch_add(1, Ordering::SeqCst) == 0 {
                gate.wait();
            }
        }),
    )
    .unwrap();

    let total: usize = 4 * 1024 * 1024;
    let producer = thread::spawn(move || {
        let block = vec![b'x'; 1024];
        let mut written = 0;
        while written < total {
            output.write_all(&block).unwrap();
            written += block.len();
        }
    });
    wait_for("the parser to stall", || first.load(Ordering::SeqCst) >= 1);
    thread::sleep(Duration::from_millis(300));
    let read_ahead = engine.stats().bytes_read - engine.stats().bytes_parsed;
    // One chunk in the parser's hands, `channel_capacity` waiting, one in the reader's hands.
    assert!(read_ahead <= 4 * 1024, "read ahead {read_ahead} bytes");
    assert!(
        engine.stats().bytes_read < total as u64 / 2,
        "the producer must be held back"
    );

    release.wait();
    producer.join().unwrap();
    wait_for("everything parsed", || {
        engine.stats().bytes_parsed == total as u64
    });
}

#[test]
fn a_program_that_stops_reading_input_cannot_stall_the_output() {
    // The "PTY" writer blocks forever, as a child that never reads its input would. It reports
    // when it has taken its first message, so the test knows exactly how full the queue is.
    struct Stuck {
        entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
        never: Arc<Barrier>,
    }
    impl Write for Stuck {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            self.never.wait();
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let never = Arc::new(Barrier::new(2));
    let mut rig = rig_with(
        EngineOptions {
            size: size(20, 5),
            input_capacity: 4,
            ..EngineOptions::default()
        },
        Box::new(Stuck {
            entered: Mutex::new(Some(entered_tx)),
            never,
        }),
        Sink::default(),
    );

    // The first message is taken by the stuck writer; exactly four more fit in the queue.
    rig.engine.send_input(b"k").unwrap();
    entered_rx
        .recv_timeout(WAIT)
        .expect("the writer takes the first message");
    for _ in 0..4 {
        rig.engine.send_input(b"k").unwrap();
    }
    let refused = rig.engine.send_input(b"k").unwrap_err();
    assert_eq!(refused.code, ErrorCode::Conflict);

    // Output still flows, and a query reply is dropped (and counted) instead of blocking.
    output(&mut rig).write_all(b"still alive\x1b[6n").unwrap();
    wait_for("output while input is stuck", || {
        rig.engine.stats().bytes_parsed == 15
    });
    assert_eq!(rig.engine.snapshot().row_text(0), "still alive");
    wait_for("the dropped reply to be counted", || {
        rig.engine.stats().dropped_replies == 1
    });
}

#[test]
fn input_after_the_session_ended_reports_not_found() {
    // A writer that fails, as a PTY whose child has gone away does.
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let rig = rig_with(
        EngineOptions {
            size: size(20, 5),
            ..EngineOptions::default()
        },
        Box::new(Closed),
        Sink::default(),
    );
    rig.engine.send_input(b"x").unwrap();
    wait_for(
        "the writer thread to stop",
        || matches!(rig.engine.send_input(b"y"), Err(error) if error.code == ErrorCode::NotFound),
    );
}

#[test]
fn find_lines_searches_history_and_scroll_to_line_brings_a_match_into_view() {
    let mut text = String::new();
    for line in 1..=100 {
        text.push_str(&format!("Line {line}\r\n"));
    }
    let rig = print_and_finish(rig(20, 10), text.as_bytes());

    // Case is ignored on request; the oldest match comes first and a limit cuts the list.
    let all = rig.engine.find_lines("line 4", true, 50);
    let texts: Vec<&str> = all.iter().map(|(_, text)| text.as_str()).collect();
    assert_eq!(texts[0], "Line 4");
    assert!(texts.contains(&"Line 40") && texts.contains(&"Line 49"));
    assert!(all[0].0 < 0, "an old line is in the history");
    assert!(rig.engine.find_lines("line 4", false, 50).is_empty());
    assert_eq!(rig.engine.find_lines("Line 4", false, 3).len(), 3);
    assert!(rig.engine.find_lines("", true, 5).is_empty());

    // Scrolling to the first match shows it.
    let (line, text) = all[0].clone();
    rig.engine.scroll_to_line(line);
    let shown = rig.engine.snapshot();
    assert!(
        (0..usize::from(shown.rows)).any(|row| shown.row_text(row) == text),
        "{text:?} is on screen"
    );
    rig.engine.scroll(ScrollRequest::Bottom);
    assert_eq!(rig.engine.snapshot().display_offset, 0);
}
