//! The editor view on GPUI's test platform with real text shaping and a real temporary project.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::AppContext as _;
use gpui::{Entity, TestAppContext, TestDispatcher, VisualTestContext};
use gpui_base::input::{EditorState, InputHighlighter, Rope};
use gpui_wgpu::CosmicTextSystem;
use rwk_core::project::ProjectRoot;
use rwk_core::workbench::Tone;
use rwk_ui::editor::highlight::{SyntaxHighlighter, HIGHLIGHT_LIMIT_BYTES};
use rwk_ui::editor::styles::SyntaxStyles;
use rwk_ui::editor::view::EditorView;

const WAIT: Duration = Duration::from_secs(10);

/// One window at a time: each test starts background work and shares the CPU.
static SERIAL: Mutex<()> = Mutex::new(());

struct Rig {
    _serial: std::sync::MutexGuard<'static, ()>,
    cx: &'static mut VisualTestContext,
    view: Entity<EditorView>,
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
    app.update(gpui_base::init);
    let (view, cx) = app.add_window_view(|window, cx| EditorView::new(Tone::Dark, window, cx));
    Rig {
        _serial: serial,
        cx,
        view,
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    project: Arc<ProjectRoot>,
}

fn fixture(files: &[(&str, &[u8])]) -> Fixture {
    let dir = tempfile::tempdir().expect("temp dir");
    for (path, content) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("directories");
        }
        std::fs::write(full, content).expect("file");
    }
    let project = Arc::new(ProjectRoot::open(dir.path().to_path_buf()).expect("project"));
    Fixture { dir, project }
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

    fn open(&mut self, fixture: &Fixture, path: &str) {
        let (view, project, path) = (self.view.clone(), fixture.project.clone(), path.to_owned());
        self.cx.update(|window, cx| {
            view.update(cx, |editor, cx| editor.open_path(project, path, window, cx))
        });
    }

    fn text(&mut self) -> String {
        let view = self.view.clone();
        self.cx.update(|_w, cx| view.read(cx).text(cx))
    }

    fn dirty(&mut self) -> bool {
        let view = self.view.clone();
        self.cx.update(|_w, cx| view.read(cx).is_dirty())
    }

    fn status(&mut self) -> Option<String> {
        let view = self.view.clone();
        self.cx.update(|_w, cx| view.read(cx).status())
    }

    fn path(&mut self) -> Option<String> {
        let view = self.view.clone();
        self.cx
            .update(|_w, cx| view.read(cx).path().map(str::to_owned))
    }

    fn type_text(&mut self, text: &str) {
        let (view, text) = (self.view.clone(), text.to_owned());
        self.cx.update(|window, cx| {
            view.update(cx, |editor, cx| editor.insert_text(&text, window, cx))
        });
        self.cx.run_until_parked();
    }

    fn save(&mut self, fixture: &Fixture) {
        let (view, project) = (self.view.clone(), fixture.project.clone());
        self.cx
            .update(|_w, cx| view.update(cx, |editor, cx| editor.save(project, cx)));
    }

    fn activate(&mut self, path: &str) {
        let (view, path) = (self.view.clone(), path.to_owned());
        self.cx.update(|window, cx| {
            view.update(cx, |editor, cx| editor.activate_tab(&path, window, cx))
        });
    }

    fn close_tab(&mut self, path: &str) {
        let (view, path) = (self.view.clone(), path.to_owned());
        self.cx
            .update(|window, cx| view.update(cx, |editor, cx| editor.close_tab(&path, window, cx)));
    }

    fn paths(&mut self) -> Vec<String> {
        let view = self.view.clone();
        self.cx.update(|_w, cx| view.read(cx).paths())
    }
}

#[test]
fn opening_a_file_shows_its_text_and_is_clean() {
    let fixture = fixture(&[("src/lib.rs", b"fn main() {}\n")]);
    let mut rig = rig();
    rig.open(&fixture, "src/lib.rs");
    assert!(rig.settle_until(|rig| rig.text() == "fn main() {}\n"));
    assert_eq!(rig.path().as_deref(), Some("src/lib.rs"));
    assert!(!rig.dirty());
}

#[test]
fn typing_marks_the_file_dirty_and_saving_writes_it_and_clears_dirty() {
    let fixture = fixture(&[("a.txt", b"hello")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "hello"));
    rig.type_text("!");
    assert!(rig.dirty());
    rig.save(&fixture);
    assert!(rig.settle_until(|rig| !rig.dirty()));
    let on_disk = std::fs::read_to_string(fixture.dir.path().join("a.txt")).expect("read");
    assert_eq!(on_disk, rig.text());
    assert!(on_disk.contains('!'));
}

#[test]
fn a_save_over_a_changed_file_reports_a_conflict_and_keeps_the_text() {
    let fixture = fixture(&[("a.txt", b"original")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "original"));
    rig.type_text("X");
    std::fs::write(fixture.dir.path().join("a.txt"), "changed elsewhere").expect("write");
    rig.save(&fixture);
    let view = rig.view.clone();
    assert!(rig.settle_until(|rig| rig.cx.update(|_w, cx| view.read(cx).has_conflict())));
    assert!(rig.dirty(), "the edits must be kept");
    assert!(rig.text().contains('X'));
    assert_eq!(
        std::fs::read_to_string(fixture.dir.path().join("a.txt")).expect("read"),
        "changed elsewhere",
        "a conflicting save must not overwrite the file"
    );
    assert!(rig.status().is_some());
}

#[test]
fn opening_a_binary_file_shows_a_message_and_keeps_the_open_file() {
    let fixture = fixture(&[("a.txt", b"text"), ("blob.bin", &[0xff, 0xfe, 0x00, 0x80])]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "text"));
    rig.open(&fixture, "blob.bin");
    assert!(rig.settle_until(|rig| rig.status().is_some()));
    assert_eq!(rig.text(), "text");
    assert_eq!(rig.path().as_deref(), Some("a.txt"));
}

#[test]
fn opening_a_file_over_two_mib_shows_a_message_and_keeps_the_open_file() {
    let big = vec![b'a'; 2 * 1024 * 1024 + 1];
    let fixture = fixture(&[("a.txt", b"text"), ("big.txt", &big)]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "text"));
    rig.open(&fixture, "big.txt");
    assert!(rig.settle_until(|rig| rig.status().is_some()));
    assert_eq!(rig.path().as_deref(), Some("a.txt"));
}

#[test]
fn opening_another_file_keeps_the_unsaved_edits_in_their_tab() {
    let fixture = fixture(&[("a.txt", b"one"), ("b.txt", b"two")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "one"));
    rig.type_text("!");
    rig.open(&fixture, "b.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("b.txt")));
    assert_eq!(rig.text(), "two");
    assert_eq!(rig.paths(), vec!["a.txt", "b.txt"]);
    // The edits are still there, in their own tab.
    rig.activate("a.txt");
    assert!(rig.text().contains('!'));
    assert!(rig.dirty());
    rig.activate("b.txt");
    assert!(!rig.dirty());
}

#[test]
fn closing_a_tab_keeps_the_others_and_moves_to_a_neighbour() {
    let fixture = fixture(&[("a.txt", b"one"), ("b.txt", b"two")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("a.txt")));
    rig.open(&fixture, "b.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("b.txt")));
    rig.close_tab("b.txt");
    assert_eq!(rig.path().as_deref(), Some("a.txt"));
    assert_eq!(rig.text(), "one");
    assert_eq!(rig.paths(), vec!["a.txt"]);
    rig.close_tab("a.txt");
    assert_eq!(rig.path().as_deref(), None);
    assert!(rig.paths().is_empty());
}

#[test]
fn a_dirty_tab_is_never_reloaded_from_disk() {
    let fixture = fixture(&[("a.txt", b"one")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "one"));
    rig.type_text("!");
    // A read for the same path would replace the buffer; the editor refuses it.
    rig.open(&fixture, "a.txt");
    rig.settle_until(|_| false);
    assert!(rig.text().contains('!'));
    assert!(rig.paths() == vec!["a.txt"], "no duplicate tab");
}

#[test]
fn a_hit_line_moves_the_cursor_and_an_open_tab_is_activated() {
    let fixture = fixture(&[("a.txt", b"one\ntwo\nthree\n")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text().contains("three")));
    let (view, project) = (rig.view.clone(), fixture.project.clone());
    rig.cx.update(|window, cx| {
        view.update(cx, |editor, cx| {
            editor.open_path_at(project, "a.txt".to_owned(), Some(3), window, cx)
        })
    });
    assert!(rig.settle_until(|rig| {
        let view = rig.view.clone();
        rig.cx
            .update(|_w, cx| view.read(cx).cursor_line(cx) == Some(3))
    }));
    assert_eq!(rig.paths(), vec!["a.txt"], "the reload keeps its tab");
}

#[test]
fn edits_made_while_a_save_runs_keep_the_file_dirty_in_the_view() {
    let fixture = fixture(&[("a.txt", b"hello")]);
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.text() == "hello"));
    rig.type_text("1");
    rig.save(&fixture);
    // The write has not been applied yet: this edit lands while the save is in flight.
    rig.type_text("2");
    rig.settle_until(|_| false);
    assert!(rig.dirty(), "the second edit was never saved");
}

#[test]
fn a_file_above_the_highlight_limit_opens_unhighlighted_with_a_note() {
    let big = "let a = 1;\n".repeat(HIGHLIGHT_LIMIT_BYTES / 11 + 100);
    let fixture = fixture(&[("big.rs", big.as_bytes()), ("small.rs", b"let a = 1;\n")]);
    let mut rig = rig();
    rig.open(&fixture, "big.rs");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("big.rs")));
    let view = rig.view.clone();
    assert!(!rig.cx.update(|_w, cx| view.read(cx).highlighting_enabled()));
    assert!(rig
        .status()
        .is_some_and(|text| text.contains("renklendirme")));
    rig.open(&fixture, "small.rs");
    // Unsaved edits are absent, so the small file may replace the big one.
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("small.rs")));
    assert!(rig.cx.update(|_w, cx| view.read(cx).highlighting_enabled()));
}

#[test]
fn a_very_long_line_opens_within_the_editor_budget() {
    let line = "x".repeat(300_000);
    let fixture = fixture(&[("long.txt", line.as_bytes())]);
    let mut rig = rig();
    let started = Instant::now();
    rig.open(&fixture, "long.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("long.txt")));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
}

fn git(dir: &std::path::Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        // The tests never read the user's git configuration (signing, hooks, templates).
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn the_diff_view_is_read_only_and_shows_added_and_removed_lines() {
    let fixture = fixture(&[("a.txt", b"one\ntwo\n")]);
    let dir = fixture.dir.path();
    assert!(git(dir, &["init", "-q"]));
    assert!(git(dir, &["add", "a.txt"]));
    assert!(git(dir, &["commit", "-q", "-m", "init"]));
    std::fs::write(dir.join("a.txt"), "one\nthree\n").expect("edit");
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("a.txt")));
    let (view, project) = (rig.view.clone(), fixture.project.clone());
    rig.cx
        .update(|window, cx| view.update(cx, |editor, cx| editor.show_diff(project, window, cx)));
    let view = rig.view.clone();
    assert!(rig.settle_until(|rig| rig.cx.update(|_w, cx| view.read(cx).diff_visible())));
    let (text, read_only) = rig.cx.update(|_w, cx| {
        let editor = view.read(cx);
        (editor.diff_text(cx), editor.diff_is_read_only(cx))
    });
    assert!(text.contains("-two"), "{text}");
    assert!(text.contains("+three"), "{text}");
    assert!(read_only);
    let hidden = AtomicBool::new(false);
    rig.cx.update(|_w, cx| {
        view.update(cx, |editor, cx| {
            editor.hide_diff(cx);
            hidden.store(!editor.diff_visible(), Ordering::SeqCst);
        })
    });
    assert!(hidden.load(Ordering::SeqCst));
}

#[test]
fn highlighting_is_ready_right_after_the_update_and_follows_edits() {
    let rig = rig();
    let source = "fn main() { let value = 1; }\n".repeat(400);
    let rope = Rope::from(source.as_str());
    let state = rig
        .cx
        .update(|window, cx| cx.new(|cx| EditorState::new(window, cx)));
    let mut highlighter = SyntaxHighlighter::new("rust").expect("rust");
    let started = Instant::now();
    rig.cx.update(|window, cx| {
        state.update(cx, |_state, cx| {
            highlighter.update(None, &rope, false, window, cx)
        })
    });
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "update took {:?}",
        started.elapsed()
    );
    let styles = SyntaxStyles::new(Tone::Dark);
    // No waiting: the colours exist as soon as `update` returned, for the whole text.
    let head = highlighter.styles(&(0..30), &styles);
    let tail = highlighter.styles(&(source.len() - 30..source.len()), &styles);
    let coloured = |styles: &[(std::ops::Range<usize>, gpui::HighlightStyle)]| {
        styles.iter().any(|(_, style)| style.color.is_some())
    };
    assert!(coloured(&head) && coloured(&tail));
    // An edit updates them: turning the first `fn` into plain text removes its colour.
    let edited = source.replacen("fn main", "xx main", 1);
    let edit = gpui_base::input::InputEdit {
        start_byte: 0,
        old_end_byte: 2,
        new_end_byte: 2,
        start_position: gpui_base::input::Point::new(0, 0),
        old_end_position: gpui_base::input::Point::new(0, 2),
        new_end_position: gpui_base::input::Point::new(0, 2),
    };
    let edited_rope = Rope::from(edited.as_str());
    rig.cx.update(|window, cx| {
        state.update(cx, |_state, cx| {
            highlighter.update(Some(edit), &edited_rope, false, window, cx)
        })
    });
    let first_word = highlighter.styles(&(0..2), &styles);
    assert!(
        !coloured(&first_word),
        "the edited word is still coloured: {first_word:?}"
    );
    assert!(coloured(
        &highlighter.styles(&(source.len() - 30..source.len()), &styles)
    ));
}

#[test]
fn the_diff_can_be_shown_side_by_side_with_aligned_columns() {
    let fixture = fixture(&[("a.txt", b"one\ntwo\nfour\n")]);
    let dir = fixture.dir.path();
    assert!(git(dir, &["init", "-q"]));
    assert!(git(dir, &["add", "a.txt"]));
    assert!(git(dir, &["commit", "-q", "-m", "init"]));
    std::fs::write(dir.join("a.txt"), "one\nthree\nfour\n").expect("edit");
    let mut rig = rig();
    rig.open(&fixture, "a.txt");
    assert!(rig.settle_until(|rig| rig.path().as_deref() == Some("a.txt")));
    let (view, project) = (rig.view.clone(), fixture.project.clone());
    rig.cx
        .update(|window, cx| view.update(cx, |editor, cx| editor.show_diff(project, window, cx)));
    let view = rig.view.clone();
    assert!(rig.settle_until(|rig| rig.cx.update(|_w, cx| view.read(cx).diff_visible())));

    rig.cx
        .update(|window, cx| view.update(cx, |editor, cx| editor.set_diff_split(true, window, cx)));

    let (left, right, split) = rig.cx.update(|_w, cx| {
        let editor = view.read(cx);
        (
            editor.diff_text(cx),
            editor.diff_right_text(cx).unwrap_or_default(),
            editor.diff_split(),
        )
    });
    assert!(split);
    assert!(left.contains("-two") && !left.contains("+three"), "{left}");
    assert!(
        right.contains("+three") && !right.contains("-two"),
        "{right}"
    );
    assert_eq!(left.lines().count(), right.lines().count());

    rig.cx.update(|window, cx| {
        view.update(cx, |editor, cx| editor.set_diff_split(false, window, cx))
    });
    let unified = rig.cx.update(|_w, cx| view.read(cx).diff_text(cx));
    assert!(
        unified.contains("-two") && unified.contains("+three"),
        "{unified}"
    );
}
