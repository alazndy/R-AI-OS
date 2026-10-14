//! Window tests of the terminal view. They open real GPUI windows, so they need a display and
//! run only when one is available (`harness = false`: GPUI owns the main thread, and one
//! process runs one application).
//!
//! They run only with `RWK_UI_DISPLAY_TESTS=1`: they need a compositor that delivers frame
//! callbacks to the new window. On the executor's machine (KDE Wayland, agent-launched process)
//! a plain GPUI window received exactly one frame in two seconds, so these tests are for the
//! reference machine (Plan 4b Task 10). `view_headless.rs` covers the same behaviour without one.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{App, AppContext, AsyncApp, WindowHandle, WindowKind, WindowOptions};
use rwk_core::error::Result as CoreResult;
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, TerminalHost, TerminalSize};
use rwk_core::terminal::{EngineOptions, TerminalEngine};
use rwk_ui::terminal::view::wake_channel;
use rwk_ui::terminal::{Appearance, FrameGate, TerminalView};

const WAIT: Duration = Duration::from_secs(5);

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
        unimplemented!("the window tests never spawn")
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

struct Rig {
    window: WindowHandle<TerminalView>,
    engine: Arc<TerminalEngine>,
    host: Arc<RecordingHost>,
    output: Option<std::io::PipeWriter>,
}

fn open(cx: &mut App) -> Rig {
    cx.activate(true);
    let (reader, output) = std::io::pipe().expect("pipe");
    let gate = Arc::new(FrameGate::new());
    let (wake, receiver) = wake_channel(&gate);
    let engine = Arc::new(
        TerminalEngine::start(
            Box::new(reader),
            Box::new(std::io::sink()),
            EngineOptions::default(),
            wake,
        )
        .expect("engine"),
    );
    let host = Arc::new(RecordingHost::default());
    let session = SessionId::new("window-test");
    let view_engine = Arc::clone(&engine);
    let view_host: Arc<dyn TerminalHost> = host.clone();
    let window = cx
        .open_window(
            WindowOptions {
                kind: WindowKind::Floating,
                inactive_frame_interval: Some(Duration::from_millis(16)),
                ..WindowOptions::default()
            },
            move |_window, cx| {
                cx.new(|cx| {
                    TerminalView::new(
                        view_engine,
                        view_host,
                        session,
                        Appearance::Dark,
                        gate,
                        receiver,
                        cx,
                    )
                })
            },
        )
        .expect("window");
    let _ = window.update(cx, |_view, window, _cx| window.activate_window());
    Rig {
        window,
        engine,
        host,
        output: Some(output),
    }
}

async fn wait_until(cx: &mut AsyncApp, mut condition: impl FnMut(&mut AsyncApp) -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < WAIT {
        if condition(cx) {
            return true;
        }
        cx.background_executor()
            .timer(Duration::from_millis(20))
            .await;
    }
    false
}

fn screen(rig: &Rig, cx: &mut AsyncApp) -> String {
    rig.window
        .update(cx, |view, _window, _cx| view.screen_text())
        .unwrap_or_default()
}

fn frames(rig: &Rig, cx: &mut AsyncApp) -> u64 {
    rig.window
        .update(cx, |view, _window, _cx| view.frame_stats().painted_frames)
        .unwrap_or(0)
}

async fn text_written_by_the_program_is_painted(cx: &mut AsyncApp) -> Result<(), String> {
    let mut rig = cx.update(open);
    rig.output
        .as_mut()
        .ok_or("no output pipe")?
        .write_all(b"hello\r\n")
        .map_err(|error| error.to_string())?;
    let painted = wait_until(cx, |cx| screen(&rig, cx) == "hello").await;
    if !painted {
        let _ = rig.window.update(cx, |_view, _window, cx| cx.notify());
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let after_manual = frames(&rig, cx);
        return Err(format!("after a manual notify frames={after_manual}; ",)
            + &format!(
                "screen was {:?}, painted frames {}, engine says {:?}, stats {:?}",
                screen(&rig, cx),
                frames(&rig, cx),
                rig.engine.snapshot().screen_text(),
                rig.engine.stats()
            ));
    }
    if frames(&rig, cx) == 0 {
        return Err("no frame was counted".into());
    }
    let grid = rig.window.update(cx, |view, _w, _cx| view.grid());
    let grid = grid.map_err(|error| error.to_string())?;
    if grid.cols < 2 || grid.rows < 2 {
        return Err(format!("grid was {grid:?}"));
    }
    if rig.host.resizes.lock().unwrap().is_empty() {
        return Err("the PTY side never received the grid".into());
    }
    drop(rig.output.take());
    Ok(())
}

async fn a_finished_session_shows_its_last_screen_and_stops_requesting_frames(
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let mut rig = cx.update(open);
    let mut output = rig.output.take().ok_or("no output pipe")?;
    output
        .write_all(b"last words\r\n")
        .map_err(|error| error.to_string())?;
    drop(output);
    if !wait_until(cx, |cx| screen(&rig, cx) == "last words").await {
        return Err(format!("screen was {:?}", screen(&rig, cx)));
    }
    if !rig.engine.wait_output_closed(WAIT) {
        return Err("the output never closed".into());
    }
    cx.background_executor()
        .timer(Duration::from_millis(300))
        .await;
    let before = frames(&rig, cx);
    cx.background_executor()
        .timer(Duration::from_millis(300))
        .await;
    let after = frames(&rig, cx);
    if after != before {
        return Err(format!("frames kept coming: {before} -> {after}"));
    }
    if screen(&rig, cx) != "last words" {
        return Err("the last screen changed".into());
    }
    Ok(())
}

fn main() {
    if std::env::var_os("RWK_UI_DISPLAY_TESTS").is_none() {
        println!("view_window: skipped (set RWK_UI_DISPLAY_TESTS=1 on a machine with a display)");
        return;
    }
    let failures: Arc<Mutex<Vec<String>>> = Arc::default();
    let collected = Arc::clone(&failures);
    gpui_platform::application().run(move |cx: &mut App| {
        cx.spawn(async move |cx| {
            let report = |name: &str, result: Result<(), String>| match &result {
                Ok(()) => println!("test {name} ... ok"),
                Err(message) => {
                    println!("test {name} ... FAILED: {message}");
                    collected.lock().unwrap().push(name.to_owned());
                }
            };
            report(
                "text_written_by_the_program_is_painted",
                text_written_by_the_program_is_painted(cx).await,
            );
            report(
                "a_finished_session_shows_its_last_screen_and_stops_requesting_frames",
                a_finished_session_shows_its_last_screen_and_stops_requesting_frames(cx).await,
            );
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    if !failures.lock().unwrap().is_empty() {
        std::process::exit(1);
    }
}
