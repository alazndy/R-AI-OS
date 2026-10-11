//! The native RAIOS workspace.
//!
//! `rwk-app [PROJECT_DIR] [--exit-after-first-frame]`. With `--exit-after-first-frame` the
//! process prints `FIRST_FRAME` once its first frame is painted and exits (used by the
//! cold-start gate).

mod persistence;

use std::io::Write;
use std::path::PathBuf;

use gpui::{App, AppContext, WindowOptions};
use rwk_core::terminal::ProgramPaths;
use rwk_ui::workbench::{Workbench, WorkbenchEvent};

/// Exits with a message instead of panicking: this is the application boundary.
fn fail(message: &str) -> ! {
    eprintln!("rwk-app: {message}");
    std::process::exit(1);
}

fn appearance_file() -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    persistence::config_home(xdg.as_deref(), home.as_deref())
        .map(|home| persistence::appearance_path(&home))
}

fn main() {
    let mut project: Option<PathBuf> = None;
    let mut exit_after_first_frame = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--exit-after-first-frame" => exit_after_first_frame = true,
            other if !other.starts_with("--") && project.is_none() => {
                project = Some(PathBuf::from(other));
            }
            other => fail(&format!("unknown argument {other:?}")),
        }
    }
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_base::init(cx);
        let path = appearance_file();
        let appearance = path
            .as_deref()
            .map(persistence::load_appearance)
            .unwrap_or_default();
        let window = match cx.open_window(WindowOptions::default(), |window, cx| {
            cx.new(|cx| Workbench::new(ProgramPaths::default(), appearance, window, cx))
        }) {
            Ok(window) => window,
            Err(error) => fail(&format!("could not open the window: {error}")),
        };
        let workbench = window
            .entity(cx)
            .unwrap_or_else(|_| fail("the window has no workbench"));
        // Closing the last window ends the application.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        // The window asks the workbench whether it may close (unsaved edits, owned sessions).
        let asker = workbench.clone();
        let _ = window.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, move |window, cx| {
                asker.update(cx, |workbench, cx| workbench.request_close(window, cx))
            });
        });
        // Persist appearance changes; a failure never blocks the application.
        let saved_path = path.clone();
        cx.subscribe(
            &workbench,
            move |_workbench, event: &WorkbenchEvent, _cx| {
                let WorkbenchEvent::AppearanceChanged(appearance) = event;
                if let Some(path) = &saved_path {
                    if let Err(error) = persistence::save_appearance(path, appearance) {
                        eprintln!("rwk-app: could not save the appearance: {error}");
                    }
                }
            },
        )
        .detach();
        if let Some(root) = project.clone() {
            let _ = window.update(cx, |workbench, window, cx| {
                workbench.open_project(root, window, cx);
            });
        }
        cx.activate(true);
        if exit_after_first_frame {
            let watched = workbench.clone();
            cx.spawn(async move |cx| {
                // The first render happens while the window is being created, before any GPU
                // work. The cold-start gate waits for the next compositor frame callback after
                // it, so the number includes the surface and renderer setup. A compositor that
                // withholds frame callbacks never lets this print, and the gate then reports
                // "not measured" instead of a flattering number.
                while watched.read_with(cx, |workbench, _| workbench.frames_rendered()) == 0 {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1))
                        .await;
                }
                let _ = cx.update_window(window.into(), |_root, window, _cx| {
                    window.on_next_frame(|_window, cx| {
                        println!("FIRST_FRAME");
                        let _ = std::io::stdout().flush();
                        cx.quit();
                    });
                });
            })
            .detach();
        }
    });
}
