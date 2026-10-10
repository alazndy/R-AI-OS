//! The terminal view against GPUI's test platform: real text shaping (cosmic-text), real engine
//! threads, no display and no compositor. Windows here are drawn by the test harness, so these
//! tests are deterministic and run everywhere `cargo test` runs.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    ClipboardItem, Modifiers, MouseMoveEvent, MouseUpEvent, TestAppContext, TestDispatcher,
    VisualTestContext,
};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::error::Result as CoreResult;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, TerminalHost, TerminalSize};
use rwk_core::terminal::{EngineOptions, TerminalEngine};
use rwk_ui::terminal::view::wake_channel;
use rwk_ui::terminal::{Appearance, FrameGate, TerminalView};

const WAIT: Duration = Duration::from_secs(5);

/// The tests run one at a time: each starts engine threads and the flood test saturates the CPU,
/// which would turn the others' timing waits into false failures.
static SERIAL: Mutex<()> = Mutex::new(());

/// A host that only records resizes; the engine here has no PTY.
#[derive(Default)]
struct RecordingHost {
    resizes: Mutex<Vec<TerminalSize>>,
}

impl TerminalHost for RecordingHost {
    fn spawn(
        &self,
        _project: &ProjectRoot,
        _kind: SessionKind,
        _prompt: Option<&str>,
        _size: TerminalSize,
    ) -> CoreResult<SessionId> {
        unimplemented!("the view tests never spawn")
    }
    fn write(&self, _id: &SessionId, _data: &[u8]) -> CoreResult<()> {
        Ok(())
    }
    fn resize(&self, _id: &SessionId, size: TerminalSize) -> CoreResult<()> {
        self.resizes.lock().unwrap().push(size);
        Ok(())
    }
    fn stop(&self, _id: &SessionId) -> CoreResult<()> {
        Ok(())
    }
    fn is_running(&self, _id: &SessionId) -> bool {
        true
    }
}

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
    /// Set by the engine's parser thread; the test thread forwards it (see `forward_wake`).
    woken: Arc<AtomicBool>,
    /// The production wake closure from `wake_channel`.
    bridge: Arc<dyn Fn() + Send + Sync>,
    _serial: std::sync::MutexGuard<'static, ()>,
    sink: Sink,
    cx: &'static mut VisualTestContext,
    view: gpui::Entity<TerminalView>,
    engine: Arc<TerminalEngine>,
    host: Arc<RecordingHost>,
    output: Option<std::io::PipeWriter>,
}

fn rig() -> Rig {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let app = Box::leak(Box::new(TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(CosmicTextSystem::new("monospace")),
    )));
    let sink = Sink::default();
    let (reader, output) = std::io::pipe().expect("pipe");
    let gate = Arc::new(FrameGate::new());
    let (bridge, receiver) = wake_channel(&gate);
    // GPUI's test dispatcher does not schedule tasks reliably when they are woken from a foreign
    // thread (the real platform dispatcher does), so the parser thread only raises a flag and the
    // test thread calls the production wake closure. Gate, channel and view task are unchanged.
    let woken = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&woken);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || flag.store(true, Ordering::Release));
    let engine = Arc::new(
        TerminalEngine::start(
            Box::new(reader),
            Box::new(sink.clone()),
            EngineOptions::default(),
            wake,
        )
        .expect("engine"),
    );
    let host = Arc::new(RecordingHost::default());
    let view_engine = Arc::clone(&engine);
    let view_host: Arc<dyn TerminalHost> = host.clone();
    let (view, cx) = app.add_window_view(move |_window, cx| {
        TerminalView::new(
            view_engine,
            view_host,
            SessionId::new("headless"),
            Appearance::Dark,
            gate,
            receiver,
            cx,
        )
    });
    Rig {
        woken,
        bridge,
        _serial: serial,
        sink,
        cx,
        view,
        engine,
        host,
        output: Some(output),
    }
}

impl Rig {
    /// Hands a wake-up the parser thread raised to the production wake closure, on this thread.
    fn forward_wake(&self) {
        if self.woken.swap(false, Ordering::AcqRel) {
            (self.bridge)();
        }
    }

    /// Lets the engine threads, the wake task and the window make progress until `done`.
    fn settle_until(&mut self, mut done: impl FnMut(&TerminalView) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(10));
            self.forward_wake();
            self.cx.run_until_parked();
            let view = self.view.clone();
            if self.cx.update(|_window, cx| done(view.read(cx))) {
                return true;
            }
        }
        false
    }

    /// Runs the executors for about `millis` milliseconds without waiting for anything.
    fn settle_for(&mut self, millis: u64) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            std::thread::sleep(Duration::from_millis(10));
            self.forward_wake();
            self.cx.run_until_parked();
        }
    }

    fn painted_frames(&mut self) -> u64 {
        let view = self.view.clone();
        self.cx
            .update(|_w, cx| view.read(cx).frame_stats().painted_frames)
    }
}

#[test]
fn text_written_by_the_program_is_painted() {
    let mut rig = rig();
    rig.output
        .as_mut()
        .expect("pipe")
        .write_all(b"hello\r\n")
        .expect("write");
    assert!(
        rig.settle_until(|view| view.screen_text() == "hello"),
        "the view never painted the output"
    );
    assert!(rig.painted_frames() >= 1);
    let view = rig.view.clone();
    let grid = rig.cx.update(|_w, cx| view.read(cx).grid());
    assert!(grid.cols >= 2 && grid.rows >= 2, "grid was {grid:?}");
    assert!(
        !rig.host.resizes.lock().unwrap().is_empty(),
        "the PTY side never received the grid"
    );
}

#[test]
fn a_finished_session_shows_its_last_screen_and_stops_requesting_frames() {
    let mut rig = rig();
    let mut output = rig.output.take().expect("pipe");
    output.write_all(b"last words\r\n").expect("write");
    drop(output);
    assert!(rig.settle_until(|view| view.screen_text() == "last words"));
    assert!(rig.engine.wait_output_closed(WAIT));
    rig.settle_for(300);
    let before = rig.painted_frames();
    std::thread::sleep(Duration::from_millis(200));
    rig.cx.run_until_parked();
    assert_eq!(
        before,
        rig.painted_frames(),
        "frames kept coming after the session ended"
    );
    let view = rig.view.clone();
    assert_eq!(
        rig.cx.update(|_w, cx| view.read(cx).screen_text()),
        "last words"
    );
}

impl Rig {
    /// Gives the view keyboard focus after its first draw.
    fn focus(&mut self) {
        self.settle_for(50);
        let view = self.view.clone();
        self.cx.update(|window, cx| {
            let handle = view.read(cx).focus_handle().clone();
            window.focus(&handle, cx);
        });
        self.cx.run_until_parked();
    }

    fn wait_for_sink(&mut self, expected: &[u8]) -> bool {
        let sink = self.sink.clone();
        let started = Instant::now();
        while started.elapsed() < WAIT {
            self.settle_for(20);
            if sink.bytes().ends_with(expected) {
                return true;
            }
        }
        false
    }
}

#[test]
fn keys_reach_the_program_as_terminal_bytes() {
    let mut rig = rig();
    rig.focus();
    rig.cx.simulate_keystrokes("a");
    assert!(rig.wait_for_sink(b"a"), "sink was {:?}", rig.sink.bytes());
    rig.cx.simulate_keystrokes("ctrl-c");
    assert!(rig.wait_for_sink(&[0x03]));
    rig.cx.simulate_keystrokes("up");
    assert!(rig.wait_for_sink(b"\x1b[A"));
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.wait_for_sink(b"\r"));
}

#[test]
fn a_keystroke_during_a_flood_reaches_the_program() {
    let mut rig = rig();
    rig.focus();
    let mut output = rig.output.take().expect("pipe");
    let flooder = std::thread::spawn(move || {
        let chunk = "flood line\r\n".repeat(512);
        for _ in 0..400 {
            if output.write_all(chunk.as_bytes()).is_err() {
                break;
            }
        }
    });
    rig.settle_for(100);
    rig.cx.simulate_keystrokes("a");
    assert!(
        rig.wait_for_sink(b"a"),
        "the keystroke never reached the program during the flood"
    );
    assert!(rig.painted_frames() > 1, "the view stopped painting");
    assert!(rig.engine.stats().bytes_read > 0, "the flood never started");
    // Draining the rest is not the point of this test: closing the engine ends the writer.
    drop(rig);
    let _ = flooder.join();
}

#[test]
fn a_click_sends_nothing_unless_the_program_asked() {
    let mut rig = rig();
    rig.focus();
    let origin = gpui::point(gpui::px(3.0), gpui::px(3.0));
    rig.cx.simulate_click(origin, Modifiers::default());
    rig.settle_for(200);
    assert!(rig.sink.bytes().is_empty(), "sent {:?}", rig.sink.bytes());
    // The program asks for SGR mouse reports.
    rig.output
        .as_mut()
        .expect("pipe")
        .write_all(b"\x1b[?1000h\x1b[?1006h")
        .expect("write");
    let view = rig.view.clone();
    let seen = rig.settle_until(|v| v.modes().mouse_report && v.modes().sgr_mouse);
    assert!(
        seen,
        "frames {} engine modes {:?} view modes {:?}",
        rig.painted_frames(),
        rig.engine.snapshot().modes,
        rig.cx.update(|_w, cx| view.read(cx).modes())
    );
    rig.cx.simulate_click(origin, Modifiers::default());
    assert!(
        rig.wait_for_sink(b"\x1b[<0;1;1m"),
        "sink was {:?}",
        rig.sink.bytes()
    );
}

#[test]
fn a_multiline_paste_waits_for_confirmation() {
    let mut rig = rig();
    rig.focus();
    rig.cx
        .update(|_w, cx| cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo".into())));
    rig.cx.simulate_keystrokes("ctrl-shift-v");
    rig.settle_for(200);
    let view = rig.view.clone();
    assert!(rig.cx.update(|_w, cx| view.read(cx).has_pending_paste()));
    assert!(rig.sink.bytes().is_empty(), "sent before confirmation");
    rig.cx.simulate_keystrokes("enter");
    assert!(
        rig.wait_for_sink(b"one\rtwo"),
        "sink was {:?}",
        rig.sink.bytes()
    );
}

#[test]
fn escape_cancels_a_multiline_paste_and_sends_nothing() {
    let mut rig = rig();
    rig.focus();
    rig.cx
        .update(|_w, cx| cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo".into())));
    rig.cx.simulate_keystrokes("ctrl-shift-v");
    rig.cx.simulate_keystrokes("escape");
    rig.settle_for(200);
    let view = rig.view.clone();
    assert!(!rig.cx.update(|_w, cx| view.read(cx).has_pending_paste()));
    assert!(rig.sink.bytes().is_empty());
}

#[test]
fn an_oversized_paste_is_refused_with_a_message() {
    let mut rig = rig();
    rig.focus();
    let big = "x".repeat(rwk_core::terminal::MAX_PASTE_BYTES + 1);
    rig.cx
        .update(|_w, cx| cx.write_to_clipboard(ClipboardItem::new_string(big)));
    rig.cx.simulate_keystrokes("ctrl-shift-v");
    rig.settle_for(200);
    let view = rig.view.clone();
    assert!(rig.cx.update(|_w, cx| view.read(cx).status()).is_some());
    assert!(rig.sink.bytes().is_empty());
}

#[test]
fn dragging_selects_text_and_ctrl_shift_c_copies_it() {
    let mut rig = rig();
    rig.focus();
    rig.output
        .as_mut()
        .expect("pipe")
        .write_all(b"hello world\r\n")
        .expect("write");
    let seen = rig.settle_until(|v| v.screen_text() == "hello world");
    assert!(
        seen,
        "frames {} engine {:?} view {:?}",
        rig.painted_frames(),
        rig.engine.snapshot().screen_text(),
        rig.cx.update(|_w, cx| rig.view.read(cx).screen_text())
    );
    let view = rig.view.clone();
    let (cell_w, _) = rig.cx.update(|_w, cx| view.read(cx).cell_size());
    let y = gpui::px(3.0);
    rig.cx.simulate_mouse_down(
        gpui::point(gpui::px(1.0), y),
        gpui::MouseButton::Left,
        Modifiers::default(),
    );
    rig.cx.simulate_event(MouseMoveEvent {
        position: gpui::point(gpui::px(cell_w * 4.6), y),
        pressed_button: Some(gpui::MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    rig.cx.simulate_event(MouseUpEvent {
        position: gpui::point(gpui::px(cell_w * 4.6), y),
        button: gpui::MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    rig.cx.simulate_keystrokes("ctrl-shift-c");
    rig.settle_for(100);
    let copied = rig
        .cx
        .update(|_w, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(copied.as_deref(), Some("hello"));
}

#[test]
fn repainting_does_not_resend_an_unchanged_grid() {
    let mut rig = rig();
    rig.output
        .as_mut()
        .expect("pipe")
        .write_all(b"one\r\n")
        .expect("write");
    assert!(rig.settle_until(|view| view.screen_text() == "one"));
    let after_first = rig.host.resizes.lock().unwrap().len();
    assert!(after_first >= 1);
    for line in ["two", "three", "four"] {
        let target = line.to_owned();
        rig.output
            .as_mut()
            .expect("pipe")
            .write_all(format!("{line}\r\n").as_bytes())
            .expect("write");
        assert!(rig.settle_until(|view| view.screen_text().ends_with(&target)));
    }
    assert!(rig.painted_frames() >= 4);
    assert_eq!(
        rig.host.resizes.lock().unwrap().len(),
        after_first,
        "an unchanged grid was sent to the PTY again"
    );
}
