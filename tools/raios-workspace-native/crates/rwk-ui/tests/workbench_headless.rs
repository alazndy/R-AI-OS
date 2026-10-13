//! The workbench on GPUI's test platform: real project, real editor, real PTY sessions.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, TestDispatcher, VisualTestContext};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::terminal::ProgramPaths;
use rwk_core::workbench::Tone;
use rwk_core::workbench::{Appearance, CommandId};
use rwk_ui::workbench::layout::LayoutPreset;
use rwk_ui::workbench::{Decision, PendingAction, Workbench, WorkbenchEvent};

const WAIT: Duration = Duration::from_secs(15);

static SERIAL: Mutex<()> = Mutex::new(());

struct Rig {
    _serial: std::sync::MutexGuard<'static, ()>,
    cx: &'static mut VisualTestContext,
    view: Entity<Workbench>,
    dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("a.txt"), "one").expect("a");
    std::fs::write(dir.path().join("b.txt"), "two").expect("b");
    let script = dir.path().join(".session.sh");
    std::fs::write(&script, "#!/bin/bash\nexec sleep 100000\n").expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let app = Box::leak(Box::new(TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        None,
        Arc::new(CosmicTextSystem::new("monospace")),
    )));
    app.update(gpui_base::init);
    let programs = ProgramPaths {
        shell: Some(script.clone()),
        raios: Some(script),
    };
    let (view, cx) = app.add_window_view(move |window, cx| {
        Workbench::new(programs, Appearance::default(), window, cx)
    });
    Rig {
        _serial: serial,
        cx,
        view,
        dir,
    }
}

impl Rig {
    fn settle_until(&mut self, mut done: impl FnMut(&mut Rig) -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < WAIT {
            std::thread::sleep(Duration::from_millis(10));
            self.cx.run_until_parked();
            if done(self) {
                return true;
            }
        }
        false
    }

    fn settle_for(&mut self, millis: u64) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            std::thread::sleep(Duration::from_millis(10));
            self.cx.run_until_parked();
        }
    }

    fn with<R>(
        &mut self,
        f: impl FnOnce(&mut Workbench, &mut gpui::Window, &mut gpui::Context<Workbench>) -> R,
    ) -> R {
        let view = self.view.clone();
        self.cx
            .update(|window, cx| view.update(cx, |wb, cx| f(wb, window, cx)))
    }

    fn read<R>(&mut self, f: impl FnOnce(&Workbench, &gpui::App) -> R) -> R {
        let view = self.view.clone();
        self.cx.update(|_w, cx| f(view.read(cx), cx))
    }

    fn open_project(&mut self) {
        let root: PathBuf = self.dir.path().to_path_buf();
        self.with(|wb, window, cx| wb.open_project(root, window, cx));
        assert!(self.settle_until(|rig| rig.read(|wb, cx| wb.state(cx).project_open)));
    }

    fn open_file(&mut self, path: &str) {
        let path = path.to_owned();
        self.with(|wb, window, cx| wb.request_open_file(path, window, cx));
    }

    fn editor_path(&mut self) -> Option<String> {
        self.read(|wb, cx| wb.editor().read(cx).path().map(str::to_owned))
    }

    fn editor_text(&mut self) -> String {
        self.read(|wb, cx| wb.editor().read(cx).text(cx))
    }

    fn type_text(&mut self, text: &str) {
        let text = text.to_owned();
        self.with(|wb, window, cx| {
            wb.editor()
                .clone()
                .update(cx, |editor, cx| editor.insert_text(&text, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn run(&mut self, id: CommandId) {
        self.with(|wb, window, cx| wb.run(id, window, cx));
    }
}

#[test]
fn the_command_dispatch_path_is_shared_by_shortcut_and_direct_calls() {
    let mut rig = rig();
    rig.with(|wb, window, cx| window.focus(wb.focus_handle(), cx));
    rig.cx.run_until_parked();
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::EditorLayout)
    );
    rig.run(CommandId::AgentsLayout);
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Agents);
    rig.cx.simulate_keystrokes("ctrl-1");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Balanced);
}

#[test]
fn a_disabled_command_does_nothing_and_reports_its_reason() {
    let mut rig = rig();
    rig.run(CommandId::Save);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("Açık dosya yok")
    );
    rig.run(CommandId::NewTerminal);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("Önce bir proje klasörü açın")
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn the_fifth_terminal_is_refused_with_a_reason() {
    let mut rig = rig();
    rig.open_project();
    for _ in 0..4 {
        rig.run(CommandId::NewTerminal);
    }
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 4);
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 4);
    assert_eq!(
        rig.read(|wb, _| wb.toast()).as_deref(),
        Some("En fazla 4 terminal açılabilir")
    );
}

#[test]
fn hiding_the_dock_keeps_the_terminals_alive() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let toast = rig.read(|wb, _| wb.toast());
    assert_eq!(toast, None, "new terminal failed");
    rig.run(CommandId::ToggleTerminal);
    assert!(!rig.read(|wb, _| wb.dock_visible()));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    rig.run(CommandId::ToggleTerminal);
    assert!(rig.read(|wb, _| wb.dock_visible()));
}

#[test]
fn switching_files_with_unsaved_edits_asks_first() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::OpenFile("b.txt".to_owned()))
    );
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    // Cancel changes nothing.
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert!(rig.editor_text().contains('!'));
}

#[test]
fn saving_then_continuing_writes_the_file_and_opens_the_next_one() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    rig.with(|wb, window, cx| wb.resolve(Decision::Save, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("b.txt")));
    let saved = std::fs::read_to_string(rig.dir.path().join("a.txt")).expect("read");
    assert!(saved.contains('!'), "{saved}");
    assert_eq!(rig.editor_text(), "two");
}

#[test]
fn discarding_drops_the_edits_and_opens_the_next_file() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("b.txt")));
    assert_eq!(
        std::fs::read_to_string(rig.dir.path().join("a.txt")).expect("read"),
        "one",
        "a discarded edit must never reach the disk"
    );
}

#[test]
fn a_failed_save_cancels_the_pending_switch_and_keeps_the_edits() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    std::fs::write(rig.dir.path().join("a.txt"), "changed elsewhere").expect("write");
    rig.open_file("b.txt");
    rig.with(|wb, window, cx| wb.resolve(Decision::Save, window, cx));
    rig.settle_for(500);
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    assert!(rig.editor_text().contains('!'));
    assert!(rig.read(|wb, _| wb.toast()).is_some());
}

#[test]
fn closing_with_unsaved_edits_asks_first() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(!allowed);
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::CloseWindow)
    );
    assert!(!rig.read(|wb, _| wb.close_confirmed()));
}

#[test]
fn closing_the_window_stops_owned_sessions() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 2);
    let allowed = rig.with(|wb, window, cx| wb.request_close(window, cx));
    assert!(allowed);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 0);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
    assert!(rig.read(|wb, _| wb.close_confirmed()));
}

#[test]
fn stopping_a_session_asks_for_confirmation() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    rig.run(CommandId::StopTerminal);
    assert!(matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::StopTerminal(_))
    ));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.running_sessions()) == 0));
}

#[test]
fn switching_projects_stops_the_old_sessions_but_allows_new_ones() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 1);
    let other = tempfile::tempdir().expect("other project");
    let root = other.path().to_path_buf();
    rig.with(|wb, window, cx| wb.open_project(root, window, cx));
    // Running sessions are stopped only after the user confirms.
    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 0));
    assert_eq!(rig.read(|wb, _| wb.running_sessions()), 0);
    rig.run(CommandId::NewTerminal);
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        None,
        "a new project must accept terminals"
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
}

fn focus_workbench(rig: &mut Rig) {
    rig.with(|wb, window, cx| window.focus(wb.focus_handle(), cx));
    rig.cx.run_until_parked();
}

#[test]
fn the_palette_filters_commands_by_label_and_shows_why_one_is_unavailable() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| palette.set_query("dosya", window, cx))
    });
    let rows = rig.read(|_wb, cx| palette.read(cx).entries().to_vec());
    assert!(!rows.is_empty());
    let save = rows
        .iter()
        .find(|entry| entry.id == CommandId::Save)
        .expect("save matches 'dosya'");
    assert_eq!(save.state.reason, Some("Açık dosya yok"));
    assert!(!save.state.enabled);
}

#[test]
fn the_palette_runs_the_selected_command_through_the_shared_path() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("editör yerleşimi", window, cx)
        })
    });
    rig.cx
        .update(|_w, cx| palette.update(cx, |palette, cx| palette.confirm(cx)));
    rig.cx.run_until_parked();
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::EditorLayout)
    );
    assert!(
        rig.read(|wb, _| wb.palette().is_none()),
        "the palette closes after running"
    );
}

#[test]
fn an_unavailable_command_stays_in_the_palette_instead_of_running() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| palette.set_query("kaydet", window, cx))
    });
    rig.cx
        .update(|_w, cx| palette.update(cx, |palette, cx| palette.confirm(cx)));
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    assert_eq!(
        rig.read(|wb, _| wb.last_command()),
        Some(CommandId::Commands)
    );
}

#[test]
fn only_the_palette_shortcut_works_while_the_palette_input_has_focus() {
    let mut rig = rig();
    focus_workbench(&mut rig);
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(
        rig.read(|wb, _| wb.layout()),
        LayoutPreset::Balanced,
        "ctrl-2 must not run"
    );
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(
        rig.read(|wb, _| wb.palette().is_none()),
        "the palette shortcut closes it"
    );
}

#[test]
fn escape_closes_the_palette() {
    let mut rig = rig();
    focus_workbench(&mut rig);
    rig.cx.simulate_keystrokes("ctrl-shift-p");
    assert!(rig.read(|wb, _| wb.palette().is_some()));
    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.palette().is_none()));
}

#[test]
fn the_unsaved_changes_dialog_offers_save_discard_and_cancel() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    rig.open_file("b.txt");
    let dialog = rig.read(|wb, _| wb.dialog()).expect("a dialog is waiting");
    let decisions: Vec<Decision> = dialog
        .buttons
        .iter()
        .map(|(decision, _)| *decision)
        .collect();
    assert_eq!(
        decisions,
        [Decision::Save, Decision::Discard, Decision::Cancel]
    );
    assert!(dialog.message.contains("b.txt"));
}

#[test]
fn escape_cancels_the_dialog_and_enter_takes_the_default_button() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    focus_workbench(&mut rig);
    rig.open_file("b.txt");
    rig.cx.simulate_keystrokes("escape");
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    assert!(
        rig.editor_text().contains('!'),
        "cancelling must change nothing"
    );
    // Enter picks the default, which is Save.
    rig.open_file("b.txt");
    rig.cx.simulate_keystrokes("enter");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("b.txt")));
    assert!(std::fs::read_to_string(rig.dir.path().join("a.txt"))
        .expect("read")
        .contains('!'));
}

#[test]
fn no_shortcut_runs_while_a_dialog_is_open() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    focus_workbench(&mut rig);
    rig.open_file("b.txt");
    assert!(rig.read(|wb, _| wb.pending().is_some()));
    rig.cx.simulate_keystrokes("ctrl-2");
    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Balanced);
    assert!(
        rig.read(|wb, _| wb.pending().is_some()),
        "the dialog stays open"
    );
}

#[test]
fn changing_the_tone_announces_the_new_appearance_and_keeps_the_terminals() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    rig.cx.update(|_w, app| {
        app.subscribe(&view, move |_wb, event: &WorkbenchEvent, _app| {
            sink.lock().unwrap().push(*event);
        })
        .detach();
    });
    rig.run(CommandId::Settings);
    let seen = events.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let WorkbenchEvent::AppearanceChanged(appearance) = seen[0];
    assert_eq!(appearance.tone, Tone::Light);
    assert_eq!(rig.read(|wb, _| wb.appearance().tone), Tone::Light);
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "switching tone must not touch the PTYs"
    );
}

#[test]
fn the_first_render_is_counted_for_the_cold_start_gate() {
    let mut rig = rig();
    rig.cx.run_until_parked();
    assert!(rig.read(|wb, _| wb.frames_rendered()) >= 1);
}

#[test]
fn switching_projects_with_running_sessions_asks_first() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::NewTerminal);
    let other = tempfile::tempdir().expect("other project");
    let root = other.path().to_path_buf();
    rig.with(|wb, window, cx| wb.open_project(root.clone(), window, cx));
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::OpenProjectStoppingSessions(root))
    );
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "nothing stopped yet"
    );
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert_eq!(
        rig.read(|wb, _| wb.running_sessions()),
        1,
        "cancel keeps the session"
    );
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 1);
}

#[test]
fn reopening_the_open_file_with_unsaved_edits_asks_instead_of_overwriting_them() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.type_text("!");
    // Clicking the file that is already open must not reload it over the edits.
    rig.open_file("a.txt");
    rig.settle_for(300);
    assert!(
        rig.editor_text().contains('!'),
        "the edits were overwritten"
    );
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::OpenFile("a.txt".to_owned()))
    );
    // Discard is the way to reload from disk, for example after a conflict.
    std::fs::write(rig.dir.path().join("a.txt"), "changed on disk").expect("write");
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_text() == "changed on disk"));
    assert!(!rig.read(|wb, cx| wb.state(cx).dirty));
}

#[test]
fn reopening_the_open_clean_file_does_nothing() {
    let mut rig = rig();
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_text() == "one"));
    rig.open_file("a.txt");
    assert!(rig.read(|wb, _| wb.pending().is_none()));
}

fn tick_ready(rig: &mut Rig) {
    while rig.cx.executor().tick() {}
}

#[test]
fn toasts_expire_after_five_seconds_and_a_replacement_gets_its_own_timer() {
    let mut rig = rig();
    rig.run(CommandId::Save);
    tick_ready(&mut rig);
    assert!(rig.read(|wb, _| wb.toast()).is_some());
    rig.cx.executor().advance_clock(Duration::from_secs(4));
    tick_ready(&mut rig);
    assert!(rig.read(|wb, _| wb.toast()).is_some());
    rig.run(CommandId::NewTerminal);
    tick_ready(&mut rig);
    let replacement = rig.read(|wb, _| wb.toast());
    assert!(replacement.is_some());
    rig.cx.executor().advance_clock(Duration::from_secs(1));
    tick_ready(&mut rig);
    assert_eq!(rig.read(|wb, _| wb.toast()), replacement);
    rig.cx.executor().advance_clock(Duration::from_secs(4));
    tick_ready(&mut rig);
    assert_eq!(rig.read(|wb, _| wb.toast()), None);
}

#[test]
fn quick_open_finds_nested_files_and_respects_unsaved_edits() {
    let mut rig = rig();
    std::fs::create_dir(rig.dir.path().join("nested")).expect("nested");
    std::fs::write(rig.dir.path().join("nested/needle.rs"), "nested").expect("file");
    std::fs::create_dir(rig.dir.path().join("target")).expect("target");
    std::fs::write(rig.dir.path().join("target/needle-hidden.rs"), "noise").expect("noise");
    rig.open_project();
    rig.open_file("a.txt");
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("a.txt")));
    rig.type_text("edited");
    rig.run(CommandId::Files);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("file palette");
    assert!(rig.settle_until(|rig| rig.read(|_, cx| !palette.read(cx).file_paths().is_empty())));
    rig.with(|_, window, cx| palette.update(cx, |p, cx| p.set_query("needle", window, cx)));
    assert_eq!(
        rig.read(|_, cx| palette.read(cx).file_paths().to_vec()),
        vec!["nested/needle.rs"]
    );
    rig.cx
        .update(|_, cx| palette.update(cx, |p, cx| p.confirm(cx)));
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    assert_eq!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::OpenFile("nested/needle.rs".into()))
    );
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    assert_eq!(rig.editor_path().as_deref(), Some("a.txt"));
    rig.run(CommandId::Files);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("file palette");
    assert!(rig.settle_until(|rig| rig.read(|_, cx| !palette.read(cx).file_paths().is_empty())));
    rig.with(|_, window, cx| palette.update(cx, |p, cx| p.set_query("needle", window, cx)));
    rig.cx
        .update(|_, cx| palette.update(cx, |p, cx| p.confirm(cx)));
    rig.with(|wb, window, cx| wb.resolve(Decision::Discard, window, cx));
    assert!(rig.settle_until(|rig| rig.editor_path().as_deref() == Some("nested/needle.rs")));
}

#[test]
fn the_inspector_command_toggles_the_panel_and_shows_the_dock() {
    let mut rig = rig();
    rig.open_project();
    rig.run(CommandId::ToggleTerminal);
    assert!(!rig.read(|wb, _| wb.dock_visible()));

    rig.run(CommandId::Inspector);

    assert!(rig.read(|wb, _| wb.inspector_visible()));
    assert!(rig.read(|wb, _| wb.dock_visible()));
    rig.run(CommandId::Inspector);
    assert!(!rig.read(|wb, _| wb.inspector_visible()));
}

#[test]
fn the_inspector_lists_only_the_records_of_the_open_project() {
    use raios_contracts::{NowSnapshot, ScoredApprovalDto, UnifiedTaskDto, WorkSnapshot};
    use rwk_core::control::fake::FakeControlPlane;
    use rwk_core::control::ControlPlane;

    let mut rig = rig();
    let root = std::fs::canonicalize(rig.dir.path()).expect("canonical root");
    let mine = root.to_string_lossy().into_owned();
    let plane = FakeControlPlane::new();
    plane
        .push_work_snapshot(Ok(WorkSnapshot {
            tasks: vec![
                UnifiedTaskDto {
                    id: "t1".into(),
                    title: "mine".into(),
                    project_path: Some(mine.clone()),
                    ..UnifiedTaskDto::default()
                },
                UnifiedTaskDto {
                    id: "t2".into(),
                    title: "elsewhere".into(),
                    project_path: Some("/somewhere/else".into()),
                    ..UnifiedTaskDto::default()
                },
            ],
            ..WorkSnapshot::default()
        }))
        .push_now_snapshot(Ok(NowSnapshot {
            approvals: vec![ScoredApprovalDto {
                id: "a1".into(),
                project_path: Some(mine),
                ..ScoredApprovalDto::default()
            }],
            ..NowSnapshot::default()
        }));
    let plane: Arc<dyn ControlPlane> = Arc::new(plane);
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));
    rig.open_project();

    let shown = rig
        .settle_until(|rig| rig.read(|wb, cx| !wb.inspector().read(cx).model().tasks.is_empty()));

    assert!(shown, "the inspector never received the snapshot");
    let (tasks, approvals) = rig.read(|wb, cx| {
        let model = wb.inspector().read(cx).model().clone();
        (model.tasks, model.approvals)
    });
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "t1");
    assert_eq!(approvals.len(), 1);
}

#[test]
fn pressing_enter_in_the_palette_runs_the_highlighted_command() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("editör yerleşimi", window, cx)
        })
    });

    rig.cx.simulate_keystrokes("enter");
    rig.cx.run_until_parked();

    assert_eq!(rig.read(|wb, _| wb.layout()), LayoutPreset::Editor);
    assert!(rig.read(|wb, _| wb.palette().is_none()));
}

#[test]
fn enter_never_adds_a_newline_to_the_palette_query() {
    let mut rig = rig();
    rig.run(CommandId::Commands);
    let palette = rig
        .read(|wb, _| wb.palette().cloned())
        .expect("palette is open");
    rig.with(|_wb, window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_query("zzzz-no-match", window, cx)
        })
    });

    rig.cx.simulate_keystrokes("enter");
    rig.cx.run_until_parked();

    let query = rig.cx.update(|_w, cx| palette.read(cx).query(cx));
    assert_eq!(query, "zzzz-no-match");
}

// ----- guarded daemon commands ---------------------------------------------------------

use rwk_core::control::fake::{FakeControlPlane, RecordedCall};
use rwk_core::control::{ControlPlane, PolicyAction, QueuedHandoff};
use rwk_core::sessions::SessionKind;

fn connected(rig: &mut Rig, policy: PolicyAction) -> Arc<FakeControlPlane> {
    let fake = Arc::new(FakeControlPlane::new());
    fake.set_policy("launch_agent", policy);
    fake.set_policy("create_handoff", policy);
    fake.set_policy("register_project", policy);
    // The first inspector poll finds the daemon healthy; only then are agent commands offered.
    fake.push_work_snapshot(Ok(raios_contracts::WorkSnapshot::default()))
        .push_now_snapshot(Ok(raios_contracts::NowSnapshot::default()));
    let plane: Arc<dyn ControlPlane> = fake.clone();
    rig.with(|wb, _, cx| wb.set_control_plane(Some(plane), cx));
    rig.open_project();
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, cx| wb.state(cx).daemon_reachable)
            && rig.read(|wb, cx| {
                wb.inspector().read(cx).model().banner == rwk_core::inspector::Banner::None
            })
    }));
    fake
}

fn sends(fake: &FakeControlPlane) -> usize {
    fake.calls()
        .iter()
        .filter(|call| {
            matches!(
                call,
                RecordedCall::LaunchAgent(_)
                    | RecordedCall::CreateHandoff(_)
                    | RecordedCall::RegisterProject { .. }
            )
        })
        .count()
}

#[test]
fn a_confirm_policy_asks_first_and_sends_only_after_yes() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Confirm);
    fake.push_launch(Ok(()));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| matches!(
        rig.read(|wb, _| wb.pending().cloned()),
        Some(PendingAction::Consent(..))
    )));
    assert_eq!(sends(&fake), 0, "nothing may be sent before the answer");
    let message = rig.read(|wb, _| wb.dialog().map(|dialog| dialog.message));
    assert!(message.is_some_and(|text| text.contains("claude_kaira")));

    rig.with(|wb, window, cx| wb.resolve(Decision::Confirm, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert_eq!(sends(&fake), 1);
}

#[test]
fn declining_the_consent_dialog_sends_nothing() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Confirm);

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Codex, window, cx));
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.pending().is_some())));
    rig.with(|wb, window, cx| wb.resolve(Decision::Cancel, window, cx));
    rig.settle_for(200);

    assert_eq!(sends(&fake), 0);
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn a_deny_policy_refuses_without_a_dialog_or_a_send() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Deny);

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.toast()).is_some()));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(sends(&fake), 0);
}

#[test]
fn an_allow_policy_sends_straight_away() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Ok(()));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Opencode, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(sends(&fake), 1);
}

#[test]
fn a_daemon_refusal_of_the_launch_starts_no_terminal() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    fake.push_launch(Err(rwk_core::error::AppError::new(
        rwk_core::error::ErrorCode::Forbidden,
        "launch refused",
    )));

    rig.with(|wb, window, cx| wb.launch_agent(SessionKind::Claude, window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.toast()).is_some()));
    assert_eq!(rig.read(|wb, _| wb.terminal_count()), 0);
}

#[test]
fn a_secret_in_the_handoff_note_never_reaches_the_daemon() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    rig.run(CommandId::Handoff);
    let composer = rig
        .read(|wb, _| wb.composer().cloned())
        .expect("the composer opens");

    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("anahtar sk-abcdefghijklmnopqrstuvwx", window, cx);
            composer.submit(SessionKind::Codex, cx);
        })
    });
    rig.cx.run_until_parked();

    assert!(rig.read(|wb, cx| wb
        .composer()
        .is_some_and(|c| c.read(cx).problem().is_some())));
    assert_eq!(sends(&fake), 0);
}

#[test]
fn a_valid_handoff_is_queued_and_tracked() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    fake.push_handoff(Ok(QueuedHandoff {
        approval_id: "a-1".into(),
        project_path: root.clone(),
    }));
    fake.push_receipt(Ok(raios_contracts::HandoffReceiptDto {
        approval_id: "a-1".into(),
        project_path: root.to_string_lossy().into_owned(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: "pending".into(),
        resolved_by: None,
        resolved_at: None,
        delivered_by_target: false,
    }));
    rig.run(CommandId::Handoff);
    let composer = rig
        .read(|wb, _| wb.composer().cloned())
        .expect("the composer opens");

    rig.with(|_, window, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_text("Devam et: testleri çalıştır.", window, cx);
            composer.submit(SessionKind::Codex, cx);
        })
    });

    assert!(rig.settle_until(|rig| !rig.read(|wb, _| wb.handoffs().is_empty())));
    assert!(
        rig.read(|wb, _| wb.composer().is_none()),
        "the composer closes"
    );
    // The receipt read that follows the queueing confirms the handoff is pending.
    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.handoffs()[0].can_start_receiver())));
    let tracked = rig.read(|wb, _| wb.handoffs()[0].clone());
    assert_eq!(tracked.approval_id, "a-1");
    assert_eq!(tracked.to, "codex_kaira");
    assert_eq!(sends(&fake), 1);
}

#[test]
fn the_agent_command_asks_which_agent() {
    let mut rig = rig();
    let _fake = connected(&mut rig, PolicyAction::Allow);

    rig.run(CommandId::Agent);

    let dialog = rig.read(|wb, _| wb.dialog()).expect("a chooser");
    assert_eq!(dialog.title, "Ajan başlat");
    assert_eq!(dialog.buttons.len(), 5);
}

#[test]
fn agent_commands_are_refused_without_a_daemon_connection() {
    let mut rig = rig();
    rig.open_project();

    rig.run(CommandId::Agent);

    assert!(rig.read(|wb, _| wb.pending().is_none()));
    assert_eq!(
        rig.read(|wb, _| wb.toast()),
        Some("Daemon bağlantısı yapılandırılmadı".to_owned())
    );
}

#[test]
fn starting_the_receiver_reads_the_receipt_again_and_tracks_the_process() {
    let mut rig = rig();
    let fake = connected(&mut rig, PolicyAction::Allow);
    let root = std::fs::canonicalize(rig.dir.path()).expect("root");
    let pending = || raios_contracts::HandoffReceiptDto {
        approval_id: "a-1".into(),
        project_path: root.to_string_lossy().into_owned(),
        from_agent: "claude_kaira".into(),
        to_agent: "codex_kaira".into(),
        status: "pending".into(),
        resolved_by: None,
        resolved_at: None,
        delivered_by_target: false,
    };
    fake.push_handoff(Ok(QueuedHandoff {
        approval_id: "a-1".into(),
        project_path: root.clone(),
    }))
    .push_receipt(Ok(pending()))
    .push_receipt(Ok(pending()))
    .push_launch(Ok(()));
    rig.with(|wb, window, cx| {
        wb.start_guarded(
            rwk_core::guarded::Guarded::CreateHandoff(rwk_core::control::CreateHandoffRequest {
                project_root: root.clone(),
                from: SessionKind::Claude,
                to: SessionKind::Codex,
                summary: "devam".into(),
                idempotency_key: rwk_core::control::IdempotencyKey::fresh(),
            }),
            window,
            cx,
        )
    });
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| {
            wb.handoffs()
                .first()
                .is_some_and(|h| h.can_start_receiver())
        })
    }));

    rig.with(|wb, window, cx| wb.start_receiver("a-1".into(), window, cx));

    assert!(rig.settle_until(|rig| rig.read(|wb, _| wb.terminal_count()) == 1));
    assert!(rig.settle_until(|rig| {
        rig.read(|wb, _| wb.handoffs()[0].label().contains("alıcı çalışıyor"))
    }));
    let reads = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, RecordedCall::HandoffReceipt { .. }))
        .count();
    assert!(
        reads >= 2,
        "the receipt is read again before the receiver starts"
    );
    assert!(
        !rig.read(|wb, _| wb.handoffs()[0].can_start_receiver()),
        "a running receiver blocks a second start"
    );
}

// ----- memory view ---------------------------------------------------------------------

struct FixedMemory;

impl rwk_core::memory::MemorySource for FixedMemory {
    fn snapshot(
        &self,
        project: &rwk_core::project::ProjectRoot,
    ) -> rwk_core::error::Result<rwk_core::memory::MemorySnapshot> {
        Ok(rwk_core::memory::MemorySnapshot {
            project_id: project.info().id.clone(),
            items: vec![rwk_core::memory::MemoryItem {
                id: "1".into(),
                slug: "persona".into(),
                title: "Persona".into(),
                description: "who".into(),
                item_type: "user".into(),
                layer: 3,
                provenance: "test".into(),
                updated_at: "2026-10-14".into(),
                body: "- [ ] todo".into(),
                truncated: false,
            }],
            total: 1,
        })
    }

    fn history(
        &self,
        _project: &rwk_core::project::ProjectRoot,
        _slug: &str,
    ) -> rwk_core::error::Result<rwk_core::memory::MemoryHistory> {
        Err(rwk_core::error::AppError::new(
            rwk_core::error::ErrorCode::NotFound,
            "none",
        ))
    }
}

#[test]
fn the_memory_view_lists_the_sections_of_memory_md_and_the_records() {
    use rwk_ui::memory::MemorySourceKind;

    let mut rig = rig();
    std::fs::write(
        rig.dir.path().join("memory.md"),
        "# Proje\n## Hedefler\n- [x] a\n- [ ] b\n",
    )
    .expect("memory.md");
    rig.with(|wb, _, _| wb.set_memory_source(Some(Arc::new(FixedMemory))));
    rig.open_project();

    rig.run(CommandId::Memory);

    let view = rig
        .read(|wb, _| wb.memory().cloned())
        .expect("the memory view opens");
    assert!(rig.settle_until(|rig| {
        rig.read(|_, cx| view.read(cx).outline(cx)) == ["Proje", "Hedefler"]
    }));
    rig.with(|_, window, cx| {
        view.update(cx, |view, cx| {
            view.show(MemorySourceKind::Records, cx);
            view.set_query("pers", window, cx);
        })
    });
    assert!(rig.settle_until(|rig| rig.read(|_, cx| view.read(cx).outline(cx)) == ["Persona"]));
}

#[test]
fn a_project_without_memory_md_says_so_and_escape_closes_the_view() {
    let mut rig = rig();
    rig.open_project();

    rig.run(CommandId::Memory);
    let view = rig.read(|wb, _| wb.memory().cloned()).expect("view");
    rig.settle_for(300);

    assert!(rig.read(|_, cx| view.read(cx).outline(cx)).is_empty());
    rig.with(|wb, window, cx| wb.close_memory(window, cx));
    assert!(rig.read(|wb, _| wb.memory().is_none()));
}

#[test]
fn the_skin_command_switches_between_standard_and_lcars_and_announces_it() {
    use rwk_core::workbench::Skin;

    let mut rig = rig();
    let events: Arc<Mutex<Vec<WorkbenchEvent>>> = Arc::default();
    let sink = Arc::clone(&events);
    let view = rig.view.clone();
    let _subscription = rig.cx.update(|_, cx| {
        cx.subscribe(&view, move |_wb, event: &WorkbenchEvent, _cx| {
            sink.lock().expect("events").push(*event);
        })
    });

    rig.run(CommandId::Skin);

    assert!(rig.read(|wb, _| wb.theme().lcars));
    let announced = events.lock().expect("events").clone();
    assert!(matches!(
        announced.last(),
        Some(WorkbenchEvent::AppearanceChanged(a)) if a.skin == Skin::Lcars
    ));
    rig.run(CommandId::Skin);
    assert!(!rig.read(|wb, _| wb.theme().lcars));
}
