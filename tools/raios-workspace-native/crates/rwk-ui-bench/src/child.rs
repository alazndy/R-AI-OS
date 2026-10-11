//! The measuring side: one scenario per process, because a GPUI application runs once per
//! process. Each child prints a single `RESULT key=value ...` line (or `FIRST_FRAME`) and exits.

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    div, App, AppContext, AsyncApp, Context, Entity, IntoElement, Keystroke, ParentElement, Render,
    Styled, Window, WindowHandle, WindowOptions,
};
use rwk_core::project::ProjectRoot;
use rwk_core::sessions::{SessionId, SessionKind, TerminalHost, TerminalSize};
use rwk_core::terminal::{ProgramPaths, PtyTerminalHost};
use rwk_ui::terminal::{Appearance, FrameGate, FrameStats, TerminalView};

use crate::stats::summarize;

/// Lines per flood session: 4M in one pane (U2), 1M in each of four panes (U3).
const FLOOD_LINES_SINGLE: u64 = 4_000_000;
const FLOOD_LINES_QUAD: u64 = 1_000_000;
/// Keystrokes timed for U4, and the pause between them.
const KEYSTROKES: usize = 200;
const KEYSTROKE_GAP: Duration = Duration::from_millis(20);
/// Idle time before PSS is read (U5).
pub(crate) const IDLE_SETTLE: Duration = Duration::from_secs(10);
/// A flood or echo that shows fewer frames than this after two seconds means the compositor is
/// not delivering frames to the window; the gate then cannot be measured.
const MIN_LIVE_FRAMES: u64 = 3;
const LIVENESS_AFTER: Duration = Duration::from_secs(2);
const TERMINAL: TerminalSize = TerminalSize {
    cols: 100,
    rows: 30,
};

/// The scenarios a child can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// Spawn to first painted frame (U1).
    ColdStart,
    /// One pane, 4M lines (U2).
    Flood,
    /// Four panes, 1M lines each (U3).
    Flood4,
    /// Echo latency of typed keys (U4).
    Keystroke,
    /// Four idle terminals (U5).
    Idle,
    /// Editor open of a 30K-line file (E1).
    EditorOpen30k,
    /// Editor open of a 500K-line buffer (E2).
    EditorOpen500k,
    /// The whole application at rest: project, file, four terminals (A2).
    AppIdle,
}

impl Scenario {
    /// Parses the command-line name.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "cold-start" => Scenario::ColdStart,
            "flood" => Scenario::Flood,
            "flood4" => Scenario::Flood4,
            "keystroke" => Scenario::Keystroke,
            "idle" => Scenario::Idle,
            "editor-open-30k" => Scenario::EditorOpen30k,
            "editor-open-500k" => Scenario::EditorOpen500k,
            "app-idle" => Scenario::AppIdle,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Scenario::ColdStart => "cold-start",
            Scenario::Flood => "flood",
            Scenario::Flood4 => "flood4",
            Scenario::Keystroke => "keystroke",
            Scenario::Idle => "idle",
            Scenario::EditorOpen30k => "editor-open-30k",
            Scenario::EditorOpen500k => "editor-open-500k",
            Scenario::AppIdle => "app-idle",
        }
    }
}

/// A view's frame gate and the sender that wakes its UI task.
type GateEntry = (Arc<FrameGate>, async_channel::Sender<()>);

/// Routes the PTY host's per-session wake-ups to the views' frame gates.
#[derive(Default)]
struct Registry {
    gates: Mutex<HashMap<SessionId, GateEntry>>,
}

impl Registry {
    fn wake(&self, id: &SessionId) {
        let found = self.gates.lock().ok().and_then(|gates| {
            gates
                .get(id)
                .map(|(gate, sender)| (gate.clone(), sender.clone()))
        });
        if let Some((gate, sender)) = found {
            if gate.request() {
                let _ = sender.try_send(());
            }
        }
    }

    fn attach(&self, id: &SessionId) -> (Arc<FrameGate>, async_channel::Receiver<()>) {
        let gate = Arc::new(FrameGate::new());
        let (sender, receiver) = async_channel::bounded::<()>(1);
        if let Ok(mut gates) = self.gates.lock() {
            gates.insert(id.clone(), (gate.clone(), sender));
        }
        (gate, receiver)
    }
}

struct Rig {
    host: Arc<PtyTerminalHost>,
    registry: Arc<Registry>,
    project: ProjectRoot,
    _dir: tempfile::TempDir,
}

fn rig(script_body: &str) -> Result<Rig, String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let script = dir.path().join("session.sh");
    write_executable(&script, &format!("#!/bin/bash\n{script_body}\n"))?;
    let project = ProjectRoot::open(dir.path().to_path_buf()).map_err(|e| format!("{e:?}"))?;
    let registry = Arc::new(Registry::default());
    let wake_registry = Arc::clone(&registry);
    let host = Arc::new(PtyTerminalHost::new(
        ProgramPaths {
            shell: Some(script),
            raios: None,
        },
        Arc::new(move |id: &SessionId| wake_registry.wake(id)),
    ));
    Ok(Rig {
        host,
        registry,
        project,
        _dir: dir,
    })
}

fn write_executable(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).map_err(|error| error.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())
}

/// Spawns, retrying while the kernel reports the freshly written script as busy.
fn spawn(rig: &Rig) -> Result<SessionId, String> {
    for _ in 0..20 {
        match rig
            .host
            .spawn(&rig.project, SessionKind::Shell, None, TERMINAL)
        {
            Ok(id) => return Ok(id),
            Err(error) if error.message.contains("Text file busy") => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(format!("{error:?}")),
        }
    }
    Err("the session script stayed busy".into())
}

/// The root view of the bench window: the panes side by side.
struct Panes {
    views: Vec<Entity<TerminalView>>,
}

impl Render for Panes {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().size_full().children(
            self.views
                .iter()
                .map(|view| div().flex_1().size_full().child(view.clone())),
        )
    }
}

/// Spawns `count` sessions and opens one window holding a view for each.
fn open_panes(
    rig: &Rig,
    count: usize,
    cx: &mut App,
) -> Result<
    (
        WindowHandle<Panes>,
        Vec<Arc<rwk_core::terminal::TerminalEngine>>,
    ),
    String,
> {
    let mut sessions = Vec::new();
    for _ in 0..count {
        let id = spawn(rig)?;
        let engine = rig.host.engine(&id).ok_or("the session vanished")?;
        let (gate, receiver) = rig.registry.attach(&id);
        sessions.push((id, engine, gate, receiver));
    }
    let engines = sessions
        .iter()
        .map(|(_, engine, _, _)| Arc::clone(engine))
        .collect();
    let host: Arc<dyn TerminalHost> = rig.host.clone();
    let handle = cx
        .open_window(WindowOptions::default(), move |_window, cx| {
            let views = sessions
                .into_iter()
                .map(|(id, engine, gate, receiver)| {
                    let host = Arc::clone(&host);
                    cx.new(|cx| {
                        TerminalView::new(engine, host, id, Appearance::Dark, gate, receiver, cx)
                    })
                })
                .collect();
            cx.new(|_| Panes { views })
        })
        .map_err(|error| error.to_string())?;
    cx.activate(true);
    Ok((handle, engines))
}

/// Frames the probe must see within [`PROBE_WINDOW`] for the window to count as live.
const PROBE_FRAMES: u32 = 10;
const PROBE_WINDOW: Duration = Duration::from_millis(1500);

/// Schedules `remaining` more next-frame callbacks, one per compositor frame.
fn pump_frames(window: &mut Window, seen: Rc<Cell<u32>>, remaining: u32) {
    window.on_next_frame(move |window, _cx| {
        seen.set(seen.get() + 1);
        if remaining > 1 {
            pump_frames(window, seen, remaining - 1);
        }
    });
}

/// True when the compositor delivers frame callbacks to the window. A window can repaint on
/// input events without that (and then holds no GPU resources), so latency and memory are
/// measured only after this passes.
pub(crate) async fn window_receives_frames(
    handle: gpui::AnyWindowHandle,
    cx: &mut AsyncApp,
) -> bool {
    let seen = Rc::new(Cell::new(0u32));
    let counter = Rc::clone(&seen);
    let started = cx.update_window(handle, |_root, window, _cx| {
        pump_frames(window, counter, PROBE_FRAMES);
    });
    if started.is_err() {
        return false;
    }
    let begun = Instant::now();
    while seen.get() < PROBE_FRAMES {
        if begun.elapsed() > PROBE_WINDOW {
            return false;
        }
        cx.background_executor()
            .timer(Duration::from_millis(5))
            .await;
    }
    true
}

fn all_stats(handle: &WindowHandle<Panes>, cx: &mut AsyncApp) -> Vec<FrameStats> {
    handle
        .update(cx, |panes, _window, cx| {
            panes
                .views
                .iter()
                .map(|v| v.read(cx).frame_stats())
                .collect()
        })
        .unwrap_or_default()
}

fn max_frames(stats: &[FrameStats]) -> u64 {
    stats.iter().map(|s| s.painted_frames).max().unwrap_or(0)
}

fn finish(line: String) {
    println!("{line}");
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

fn fail(message: &str) -> ! {
    eprintln!("ui-bench child: {message}");
    std::process::exit(2);
}

/// Runs one scenario to completion and exits the process.
pub fn run(scenario: Scenario) -> ! {
    if matches!(
        scenario,
        Scenario::EditorOpen30k | Scenario::EditorOpen500k | Scenario::AppIdle
    ) {
        crate::app_child::run(scenario);
    }
    let body: &str = match scenario {
        Scenario::ColdStart | Scenario::Idle | Scenario::AppIdle => "exec sleep 100000",
        Scenario::Keystroke => "exec cat",
        Scenario::Flood => "seq 1 4000000",
        Scenario::Flood4 => "seq 1 1000000",
        Scenario::EditorOpen30k | Scenario::EditorOpen500k => "exec sleep 100000",
    };
    let rig = match rig(body) {
        Ok(rig) => rig,
        Err(message) => fail(&message),
    };
    gpui_platform::application().run(move |cx: &mut App| {
        let panes = match scenario {
            Scenario::Flood4 | Scenario::Idle => 4,
            _ => 1,
        };
        let started = Instant::now();
        let (handle, engines) = match open_panes(&rig, panes, cx) {
            Ok(opened) => opened,
            Err(message) => fail(&message),
        };
        cx.spawn(async move |cx| {
            let line = measure(scenario, &rig, &handle, &engines, started, cx).await;
            match line {
                Ok(line) => finish(line),
                Err(message) => fail(&message),
            }
        })
        .detach();
    });
    std::process::exit(0)
}

async fn measure(
    scenario: Scenario,
    _rig: &Rig,
    handle: &WindowHandle<Panes>,
    engines: &[Arc<rwk_core::terminal::TerminalEngine>],
    started: Instant,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    match scenario {
        Scenario::ColdStart => {
            while max_frames(&all_stats(handle, cx)) == 0 {
                if started.elapsed() > Duration::from_secs(30) {
                    return Err("no frame within 30 s".into());
                }
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            }
            Ok("FIRST_FRAME".to_owned())
        }
        Scenario::Flood | Scenario::Flood4 => flood(scenario, handle, engines, started, cx).await,
        Scenario::Keystroke => keystroke(handle, cx).await,
        Scenario::Idle => idle(handle, cx).await,
        Scenario::EditorOpen30k | Scenario::EditorOpen500k | Scenario::AppIdle => {
            Err("handled by app_child".to_owned())
        }
    }
}

async fn flood(
    scenario: Scenario,
    handle: &WindowHandle<Panes>,
    engines: &[Arc<rwk_core::terminal::TerminalEngine>],
    started: Instant,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let lines = if scenario == Scenario::Flood {
        FLOOD_LINES_SINGLE
    } else {
        FLOOD_LINES_QUAD
    };
    let mut checked_liveness = false;
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(20))
            .await;
        let stats = all_stats(handle, cx);
        let elapsed = started.elapsed();
        let closed = engines.iter().all(|engine| engine.is_output_closed());
        if !checked_liveness && elapsed > LIVENESS_AFTER && !closed {
            checked_liveness = true;
            if max_frames(&stats) < MIN_LIVE_FRAMES {
                return Ok(format!(
                    "RESULT scenario={} status=noframes frames={}",
                    scenario.name(),
                    max_frames(&stats)
                ));
            }
        }
        if closed {
            let frames = max_frames(&stats);
            let slow = stats.iter().map(|s| s.slow_frames).max().unwrap_or(0);
            let seconds = elapsed.as_secs_f64();
            return Ok(format!(
                "RESULT scenario={} status=ok fps={:.2} frames={frames} slow={slow} seconds={seconds:.3} lines={lines}",
                scenario.name(),
                frames as f64 / seconds
            ));
        }
        if elapsed > Duration::from_secs(180) {
            return Err("a flood did not finish within 180 s".into());
        }
    }
}

fn screen_count(handle: &WindowHandle<Panes>, cx: &mut AsyncApp) -> (usize, Option<Instant>) {
    handle
        .update(cx, |panes, _window, cx| {
            let view = panes.views[0].read(cx);
            (
                view.screen_text().matches('x').count(),
                view.frame_stats().last_change_at,
            )
        })
        .unwrap_or((0, None))
}

async fn keystroke(handle: &WindowHandle<Panes>, cx: &mut AsyncApp) -> Result<String, String> {
    // Focus the only view and let the first frames settle.
    handle
        .update(cx, |panes, window, cx| {
            let focus = panes.views[0].read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        })
        .map_err(|error| error.to_string())?;
    // An input event repaints the window even when the compositor delivers no frame callbacks,
    // which would make the echo latency look fine; prove real frame delivery first.
    if !window_receives_frames((*handle).into(), cx).await {
        return Ok("RESULT scenario=keystroke status=noframes".to_owned());
    }
    let mut samples = Vec::with_capacity(KEYSTROKES);
    for _ in 0..KEYSTROKES {
        let (before, _) = screen_count(handle, cx);
        let sent = Instant::now();
        cx.update_window((*handle).into(), |_root, window, cx| {
            if let Ok(keystroke) = Keystroke::parse("x") {
                window.dispatch_keystroke(keystroke, cx);
            }
        })
        .map_err(|error| error.to_string())?;
        let echoed = loop {
            cx.background_executor()
                .timer(Duration::from_micros(500))
                .await;
            let (count, changed_at) = screen_count(handle, cx);
            if count > before {
                break changed_at;
            }
            if sent.elapsed() > Duration::from_secs(1) {
                return Ok("RESULT scenario=keystroke status=noecho".to_owned());
            }
        };
        if let Some(painted) = echoed {
            samples.push(painted.saturating_duration_since(sent).as_micros() as u64);
        }
        cx.background_executor().timer(KEYSTROKE_GAP).await;
    }
    let summary = summarize(samples);
    Ok(format!(
        "RESULT scenario=keystroke status=ok p50_ms={:.3} p99_ms={:.3} max_ms={:.3} samples={}",
        summary.p50_us as f64 / 1000.0,
        summary.p99_us as f64 / 1000.0,
        summary.max_us as f64 / 1000.0,
        summary.count
    ))
}

async fn idle(handle: &WindowHandle<Panes>, cx: &mut AsyncApp) -> Result<String, String> {
    // Memory measured in a window that never rendered is meaningless (no GPU resources exist
    // yet), so first prove the compositor delivers frames to it.
    handle
        .update(cx, |panes, window, cx| {
            let focus = panes.views[0].read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        })
        .map_err(|error| error.to_string())?;
    if !window_receives_frames((*handle).into(), cx).await {
        return Ok("RESULT scenario=idle status=noframes".to_owned());
    }
    cx.background_executor().timer(IDLE_SETTLE).await;
    let frames = max_frames(&all_stats(handle, cx));
    let pss = pss_mb().ok_or("could not read /proc/self/smaps_rollup")?;
    Ok(format!(
        "RESULT scenario=idle status=ok pss_mb={pss:.1} frames={frames}"
    ))
}

/// Proportional set size of this process in MB, from `/proc/self/smaps_rollup`.
pub(crate) fn pss_mb() -> Option<f64> {
    let text = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
    let line = text.lines().find(|line| line.starts_with("Pss:"))?;
    let kilobytes: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes / 1024.0)
}

/// Where the benchmark binary lives, for spawning children.
pub fn current_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| error.to_string())
}
