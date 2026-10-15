//! The P3 scenarios: the editor opening a large buffer (E1, E2) and the whole application at
//! rest (A2). Each runs in its own process like the terminal scenarios in `child.rs`.

use std::cell::Cell;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    div, App, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions,
};
use rwk_core::documents::{Document, Revision};
use rwk_core::project::ProjectId;
use rwk_core::terminal::ProgramPaths;
use rwk_core::workbench::{Appearance, CommandId, Tone};
use rwk_ui::editor::view::EditorView;
use rwk_ui::workbench::Workbench;

use crate::child::{pss_mb, window_receives_frames, Scenario, IDLE_SETTLE};

/// Lines of the two editor-open buffers.
const LINES_30K: usize = 30_000;
const LINES_500K: usize = 500_000;
/// How long the first frame after `open` may take before the run is abandoned.
const OPEN_DEADLINE: Duration = Duration::from_secs(60);

/// Window root that hosts one editor.
struct EditorRoot {
    editor: Entity<EditorView>,
}

impl Render for EditorRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.editor.clone())
    }
}

fn fail(message: &str) -> ! {
    eprintln!("ui-bench child: {message}");
    std::process::exit(2);
}

fn finish(line: String) -> ! {
    println!("{line}");
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

/// Rust-like text of `lines` lines, about 31 bytes each (30K lines stay under the 1 MiB
/// highlight limit, as a large real source file would).
fn buffer(lines: usize) -> String {
    let mut text = String::with_capacity(lines * 32);
    for line in 0..lines {
        text.push_str(&format!("    let value_{line:05} = call({line});\n"));
    }
    text
}

/// Runs one P3 scenario and exits the process.
pub fn run(scenario: Scenario) -> ! {
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_base::init(cx);
        match scenario {
            Scenario::EditorOpen30k => editor_open(scenario, LINES_30K, cx),
            Scenario::EditorOpen500k => editor_open(scenario, LINES_500K, cx),
            Scenario::AppIdle => app_idle(cx),
            _ => fail("not a P3 scenario"),
        }
    });
    std::process::exit(0)
}

fn editor_open(scenario: Scenario, lines: usize, cx: &mut App) {
    let handle = match cx.open_window(WindowOptions::default(), |window, cx| {
        let editor = cx.new(|cx| EditorView::new(Tone::Dark, window, cx));
        cx.new(|_| EditorRoot { editor })
    }) {
        Ok(handle) => handle,
        Err(error) => fail(&format!("could not open the window: {error}")),
    };
    cx.activate(true);
    let name = scenario_name(scenario);
    cx.spawn(async move |cx| {
        if !window_receives_frames(handle.into(), cx).await {
            finish(format!("RESULT scenario={name} status=noframes"));
        }
        let Ok(editor) = handle.update(cx, |root, _window, _cx| root.editor.clone()) else {
            fail("the window is gone");
        };
        let content = buffer(lines);
        let document = Document {
            project_id: ProjectId::new("bench"),
            path: "bench.rs".to_owned(),
            revision: Revision::of(content.as_bytes()),
            content,
        };
        let done: Rc<Cell<Option<Duration>>> = Rc::default();
        let signal = Rc::clone(&done);
        let started = Instant::now();
        let opened = cx.update_window(handle.into(), |_root, window, cx| {
            editor.update(cx, |editor, cx| editor.open_document(document, window, cx));
            window.on_next_frame(move |_window, _cx| signal.set(Some(started.elapsed())));
        });
        if opened.is_err() {
            fail("the window is gone");
        }
        while done.get().is_none() {
            if started.elapsed() > OPEN_DEADLINE {
                finish(format!("RESULT scenario={name} status=noframes"));
            }
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
        }
        let elapsed = done.get().unwrap_or_default();
        finish(format!(
            "RESULT scenario={name} status=ok ms={:.1} lines={lines}",
            elapsed.as_secs_f64() * 1000.0
        ));
    })
    .detach();
}

fn scenario_name(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::EditorOpen30k => "editor-open-30k",
        Scenario::EditorOpen500k => "editor-open-500k",
        _ => "app-idle",
    }
}

fn app_idle(cx: &mut App) {
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => fail(&format!("no temporary project: {error}")),
    };
    let script = dir.path().join(".session.sh");
    let written = std::fs::write(&script, "#!/bin/bash\nexec sleep 100000\n")
        .and_then(|()| std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)));
    if let Err(error) = written {
        fail(&format!("could not write the session script: {error}"));
    }
    let source = dir.path().join("src");
    let seeded = std::fs::create_dir_all(&source)
        .and_then(|()| std::fs::write(source.join("main.rs"), buffer(2_000)));
    if let Err(error) = seeded {
        fail(&format!("could not seed the project: {error}"));
    }
    let programs = ProgramPaths {
        shell: Some(script),
        raios: None,
    };
    let handle = match cx.open_window(WindowOptions::default(), move |window, cx| {
        cx.new(|cx| Workbench::new(programs, Appearance::default(), window, cx))
    }) {
        Ok(handle) => handle,
        Err(error) => fail(&format!("could not open the window: {error}")),
    };
    cx.activate(true);
    let root = dir.path().to_path_buf();
    cx.spawn(async move |cx| {
        // Keep the temporary project alive for the whole run.
        let _dir = dir;
        let _ = handle.update(cx, |workbench, window, cx| {
            workbench.open_project(root, window, cx);
        });
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let _ = handle.update(cx, |workbench, window, cx| {
            workbench.request_open_file("src/main.rs".to_owned(), window, cx);
            for _ in 0..4 {
                workbench.run(CommandId::NewTerminal, window, cx);
            }
        });
        if !window_receives_frames(handle.into(), cx).await {
            finish("RESULT scenario=app-idle status=noframes".to_owned());
        }
        cx.background_executor().timer(IDLE_SETTLE).await;
        let Some(pss) = pss_mb() else {
            fail("could not read /proc/self/smaps_rollup");
        };
        finish(format!(
            "RESULT scenario=app-idle status=ok pss_mb={pss:.1}"
        ));
    })
    .detach();
}
