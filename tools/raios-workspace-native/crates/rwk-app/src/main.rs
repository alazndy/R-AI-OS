//! The native RAIOS workspace.
//!
//! `rwk-app [PROJECT_DIR] [--open FILE] [--terminal] [--exit-after-first-frame]`. `--open` opens a
//! file of the project (a path relative to it) and `--terminal` starts one terminal, both right
//! after the project is shown. With `--exit-after-first-frame` the
//! process prints `FIRST_FRAME` once its first frame is painted and exits (used by the
//! cold-start gate).

mod persistence;
mod token;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext, WindowOptions};
use rwk_core::control::daemon::DaemonClient;
use rwk_core::control::stream::WebSocketEvents;
use rwk_core::control::ControlPlane;
use rwk_core::handoff_input::redact_secrets;
use rwk_core::memory::{MemorySource, SqliteMemorySource};
use rwk_core::search::CliSearch;
use rwk_core::terminal::ProgramPaths;
use rwk_core::usage::CliUsage;
use rwk_ui::workbench::{Workbench, WorkbenchEvent};
use token::FileTokenSource;

/// Exits with a message instead of panicking: this is the application boundary.
fn fail(message: &str) -> ! {
    eprintln!("rwk-app: {message}");
    std::process::exit(1);
}

fn config_home() -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    persistence::config_home(xdg.as_deref(), home.as_deref())
}

fn appearance_file() -> Option<PathBuf> {
    config_home().map(|home| persistence::appearance_path(&home))
}

fn main() {
    let mut project: Option<PathBuf> = None;
    let mut exit_after_first_frame = false;
    let mut open_file: Option<String> = None;
    let mut open_terminal = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--exit-after-first-frame" => exit_after_first_frame = true,
            "--terminal" => open_terminal = true,
            "--open" => {
                open_file = Some(
                    arguments
                        .next()
                        .unwrap_or_else(|| fail("--open needs a path")),
                );
            }
            other if !other.starts_with("--") && project.is_none() => {
                project = Some(PathBuf::from(other));
            }
            other => fail(&format!("unknown argument {other:?}")),
        }
    }
    gpui_platform::application()
        .with_assets(rwk_ui::assets::Assets)
        .run(move |cx: &mut App| {
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
            let registered_file = config_home().map(|home| persistence::registered_path(&home));
            if let Some(file) = &registered_file {
                let known = persistence::load_registered(file);
                let _ = window.update(cx, |workbench, _, _| workbench.set_known_projects(known));
            }
            // Per-project workspace state: editor tabs, layout and sessions are stored so the
            // next start reopens a project where it was left.
            let workspace_store =
                config_home().map(|home| Arc::new(persistence::FsWorkspaceStore::new(home)));
            if let Some(store) = &workspace_store {
                let source: Arc<dyn rwk_core::workbench::WorkspaceStateSource> = store.clone();
                workbench.update(cx, |workbench, _| {
                    workbench.set_workspace_source(Some(source))
                });
            }
            if let Some(home) = config_home() {
                rwk_core::lsp::set_user_servers(persistence::load_user_servers(
                    &persistence::servers_path(&home),
                ));
                let source: Arc<dyn rwk_core::runner::TaskSource> =
                    Arc::new(persistence::FsTaskStore::new(&home));
                workbench.update(cx, |workbench, _| workbench.set_task_source(Some(source)));
            }
            let preferences_file = config_home().map(|home| persistence::preferences_path(&home));
            if let Some(file) = &preferences_file {
                let stored = persistence::load_preferences(file);
                workbench.update(cx, |workbench, cx| {
                    workbench.set_editing_preferences(stored.auto_save, stored.format_on_save);
                    workbench.set_toolbar_layout(stored.toolbar);
                    workbench.set_editor_prefs(stored.editor, cx);
                });
            }
            let recent_file = config_home().map(|home| persistence::recent_path(&home));
            if let Some(file) = &recent_file {
                let stored = persistence::load_recent(file);
                workbench.update(cx, |workbench, _| workbench.set_recent_projects(stored));
            }
            let workspace_store_for_events = workspace_store.clone();
            cx.subscribe(
                &workbench,
                move |_workbench, event: &WorkbenchEvent, _cx| match event {
                    WorkbenchEvent::AppearanceChanged(appearance) => {
                        if let Some(path) = &saved_path {
                            if let Err(error) = persistence::save_appearance(path, appearance) {
                                eprintln!("rwk-app: could not save the appearance: {error}");
                            }
                        }
                    }
                    WorkbenchEvent::RecentProjectsChanged(projects) => {
                        if let Some(file) = &recent_file {
                            if let Err(error) = persistence::save_recent(file, projects) {
                                eprintln!("rwk-app: could not save the recent projects: {error}");
                            }
                        }
                    }
                    WorkbenchEvent::ProjectRegistered(project) => {
                        if let Some(file) = &registered_file {
                            if let Err(error) = persistence::remember_registered(file, project) {
                                eprintln!("rwk-app: could not remember the registration: {error}");
                            }
                        }
                    }
                    WorkbenchEvent::PreferencesChanged {
                        auto_save,
                        format_on_save,
                        toolbar,
                        editor,
                    } => {
                        if let Some(file) = &preferences_file {
                            let preferences = persistence::Preferences {
                                auto_save: *auto_save,
                                format_on_save: *format_on_save,
                                toolbar: *toolbar,
                                editor: *editor,
                            };
                            if let Err(error) = persistence::save_preferences(file, preferences) {
                                eprintln!("rwk-app: could not save the preferences: {error}");
                            }
                        }
                    }
                    WorkbenchEvent::WorkspaceChanged(workspace) => {
                        if let Some(store) = &workspace_store_for_events {
                            if let Err(error) = store.save(workspace) {
                                eprintln!("rwk-app: could not save the workspace: {error}");
                            }
                        }
                    }
                },
            )
            .detach();
            // The structured memory records: a read-only view of the daemon's database, with
            // credentials hidden before anything is shown.
            if let Some(config_home) = config_home() {
                let source =
                    SqliteMemorySource::new(config_home.join("raios").join("workspace.db"))
                        .with_redactor(redact_secrets);
                let source: Arc<dyn MemorySource> = Arc::new(source);
                workbench.update(cx, |workbench, _| workbench.set_memory_source(Some(source)));
            }
            // Project search shells out to the `raios` CLI's trigram index, so it works with or
            // without the daemon; without the CLI on PATH the view says so when a search runs.
            workbench.update(cx, |workbench, _| {
                workbench.set_search_source(Some(Arc::new(CliSearch::default())));
                workbench.set_language_server_launcher(Some(rwk_core::lsp::default_launcher()));
            });
            // The quota signal: `raios usage --json` read on the background executor and
            // refreshed on a timer; the status bar stays quiet until a reading lands.
            workbench.update(cx, |workbench, cx| {
                workbench.set_usage_source(Some(Arc::new(CliUsage::default())), cx);
            });
            // The daemon connection: without a config home there is no token file, so the inspector
            // keeps showing "not configured" instead of guessing a path. The event stream travels
            // on the same base URL and bearer token; the inspector keeps polling either way.
            if let Some(config_home) = config_home() {
                let base = format!("http://127.0.0.1:{}", DaemonClient::DEFAULT_PORT);
                let client = DaemonClient::new(
                    base.clone(),
                    Box::new(FileTokenSource::new(token::token_path(&config_home))),
                );
                let plane: Arc<dyn ControlPlane> = Arc::new(client);
                let events = WebSocketEvents::new(
                    &base,
                    Box::new(FileTokenSource::new(token::token_path(&config_home))),
                );
                workbench.update(cx, |workbench, cx| {
                    workbench.set_control_plane(Some(plane), cx);
                    workbench.set_event_source(Some(Box::new(events)), cx);
                });
            }
            if let Some(root) = project.clone() {
                let _ = window.update(cx, |workbench, window, cx| {
                    workbench.open_project(root, window, cx);
                });
                if open_file.is_some() || open_terminal {
                    // The project and its listing are ready a moment after `open_project`.
                    let file = open_file.clone();
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(400))
                            .await;
                        let _ = window.update(cx, |workbench, window, cx| {
                            if let Some(path) = file {
                                workbench.request_open_file(path, window, cx);
                            }
                            if open_terminal {
                                workbench.run(
                                    rwk_core::workbench::CommandId::NewTerminal,
                                    window,
                                    cx,
                                );
                            }
                        });
                    })
                    .detach();
                }
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
