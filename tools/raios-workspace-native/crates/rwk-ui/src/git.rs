//! The source control view: the repository's changed files, staging, the commit message and the
//! branch list. It only shows state and asks; the workbench runs git and sends the new status.

use gpui::{
    div, px, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::git::{Entry, FileDiff, Stash, Status};
use rwk_core::workbench::{Appearance, Theme};

/// What the view asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitEvent {
    /// Read the status again.
    Refresh,
    /// Stage these files.
    Stage(Vec<String>),
    /// Unstage these files.
    Unstage(Vec<String>),
    /// Throw away the working-tree changes of these files (already confirmed in the view).
    Discard(Vec<Entry>),
    /// Commit what is staged with this message.
    Commit(String),
    /// Switch to this branch.
    Switch(String),
    /// Open this file in the editor.
    Open(String),
    /// Fast-forward from the upstream.
    Pull,
    /// Push the current branch (already confirmed in the view).
    Push,
    /// Stash the changes with this message.
    StashPush(String),
    /// Apply and drop this stash entry.
    StashPop(usize),
    /// Create a branch with this name and switch to it.
    CreateBranch(String),
    /// Delete this merged branch.
    DeleteBranch(String),
    /// Read the hunks of this file (`staged`: those in the index) for the hunk panel.
    Hunks {
        /// Project-relative path.
        path: String,
        /// Show what is staged instead of what is not.
        staged: bool,
    },
    /// Stage (or unstage) one hunk; `header` is the hunk as shown.
    ApplyHunk {
        /// Project-relative path.
        path: String,
        /// Position in the shown diff.
        index: usize,
        /// The `@@` line shown, to refuse a hunk that moved.
        header: String,
        /// Stage it (otherwise unstage).
        stage: bool,
    },
    /// Close the view.
    Close,
}

impl EventEmitter<GitEvent> for GitView {}

/// The hunks of one file on screen.
struct HunkPanel {
    path: String,
    staged: bool,
    /// `None` until the diff arrives.
    diff: Option<FileDiff>,
}

/// The source control view.
pub struct GitView {
    project_name: String,
    status: Option<Status>,
    branches: Vec<String>,
    /// The last failure, shown until the next successful action.
    error: Option<String>,
    busy: bool,
    /// Files waiting for the answer to "really discard?".
    confirm: Option<Vec<Entry>>,
    hunks: Option<HunkPanel>,
    stashes: Vec<Stash>,
    /// The push waits for a yes.
    confirm_push: bool,
    message: Entity<InputState>,
    /// The branch name or stash note, apart from the commit message.
    detail: Entity<InputState>,
    theme: Theme,
    _subscription: Subscription,
}

impl GitView {
    /// A view for `project_name`; the message box takes focus.
    pub fn new(project_name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| {
            InputState::new(window, cx)
                .submit_on_enter(true)
                .placeholder("Commit mesajı")
        });
        let detail = cx.new(|cx| InputState::new(window, cx).placeholder("Dal adı / saklama notu"));
        message.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe(&message, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.commit(cx);
            }
        });
        Self {
            project_name,
            status: None,
            branches: Vec::new(),
            error: None,
            busy: true,
            confirm: None,
            hunks: None,
            stashes: Vec::new(),
            confirm_push: false,
            message,
            detail,
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// Repaints the view with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Shows a fresh status (and clears the failure of the action before it).
    pub fn set_status(&mut self, status: Status, branches: Vec<String>, cx: &mut Context<Self>) {
        self.status = Some(status);
        self.branches = branches;
        self.busy = false;
        self.confirm = None;
        // An open hunk panel shows what the action just changed.
        if let Some(panel) = &self.hunks {
            cx.emit(GitEvent::Hunks {
                path: panel.path.clone(),
                staged: panel.staged,
            });
        }
        cx.notify();
    }

    /// Shows the hunks of `path` (leaving the file list until [`Self::close_hunks`]).
    pub fn set_hunks(&mut self, path: &str, staged: bool, diff: FileDiff, cx: &mut Context<Self>) {
        self.hunks = Some(HunkPanel {
            path: path.to_owned(),
            staged,
            diff: Some(diff),
        });
        cx.notify();
    }

    /// Opens the hunk panel for `path`; the diff arrives through [`Self::set_hunks`].
    pub fn open_hunks(&mut self, path: &str, staged: bool, cx: &mut Context<Self>) {
        self.hunks = Some(HunkPanel {
            path: path.to_owned(),
            staged,
            diff: None,
        });
        cx.emit(GitEvent::Hunks {
            path: path.to_owned(),
            staged,
        });
        cx.notify();
    }

    /// Goes back from the hunk panel to the file list.
    pub fn close_hunks(&mut self, cx: &mut Context<Self>) {
        self.hunks = None;
        cx.notify();
    }

    /// The hunks on screen: file, whether they are the staged ones, and the diff.
    pub fn hunk_panel(&self) -> Option<(&str, bool, Option<&FileDiff>)> {
        self.hunks
            .as_ref()
            .map(|panel| (panel.path.as_str(), panel.staged, panel.diff.as_ref()))
    }

    /// Stages or unstages hunk `index` of the open panel.
    pub fn apply_hunk(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(panel) = &self.hunks else {
            return;
        };
        let Some(hunk) = panel.diff.as_ref().and_then(|diff| diff.hunks.get(index)) else {
            return;
        };
        if self.busy {
            return;
        }
        cx.emit(GitEvent::ApplyHunk {
            path: panel.path.clone(),
            index,
            header: hunk.header.clone(),
            stage: !panel.staged,
        });
    }

    /// Shows why the last action or status read failed.
    pub fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        self.busy = false;
        cx.notify();
    }

    /// Marks an action as running (buttons are inert meanwhile).
    pub fn set_busy(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
    }

    /// Empties the message box (after a successful commit).
    pub fn clear_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.message
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// Replaces the message text (as typing would).
    pub fn set_message(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.message
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// Replaces the branch name / stash note text.
    pub fn set_detail(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.detail
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// The status on screen.
    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// The failure on screen.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether an action is running.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Whether the "really discard?" question is showing.
    pub fn confirming(&self) -> bool {
        self.confirm.is_some()
    }

    /// Stages every file with unstaged changes.
    pub fn stage_all(&mut self, cx: &mut Context<Self>) {
        let Some(status) = &self.status else {
            return;
        };
        let paths: Vec<String> = status.changes().map(|entry| entry.path.clone()).collect();
        if !paths.is_empty() && !self.busy {
            cx.emit(GitEvent::Stage(paths));
        }
    }

    /// Asks to discard the changes of `entries`; nothing happens until it is confirmed.
    pub fn ask_discard(&mut self, entries: Vec<Entry>, cx: &mut Context<Self>) {
        if !entries.is_empty() && !self.busy {
            self.confirm = Some(entries);
            cx.notify();
        }
    }

    /// Answers the "really discard?" question.
    pub fn answer_discard(&mut self, yes: bool, cx: &mut Context<Self>) {
        if let Some(entries) = self.confirm.take() {
            if yes {
                cx.emit(GitEvent::Discard(entries));
            }
        }
        cx.notify();
    }

    /// Shows the stash entries.
    pub fn set_stashes(&mut self, stashes: Vec<Stash>, cx: &mut Context<Self>) {
        self.stashes = stashes;
        cx.notify();
    }

    /// The stash entries on screen.
    pub fn stashes(&self) -> &[Stash] {
        &self.stashes
    }

    /// Asks whether to push; the answer arrives through [`Self::answer_push`].
    pub fn ask_push(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.confirm_push = true;
            cx.notify();
        }
    }

    /// Whether the "really push?" question is showing.
    pub fn confirming_push(&self) -> bool {
        self.confirm_push
    }

    /// Answers the "really push?" question.
    pub fn answer_push(&mut self, yes: bool, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.confirm_push) && yes {
            cx.emit(GitEvent::Push);
        }
        cx.notify();
    }

    /// Stashes the changes under the note in the second box (or a default one).
    pub fn stash(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.detail.read(cx).value().to_string();
        let text = if text.trim().is_empty() {
            "RAIOS".to_owned()
        } else {
            text
        };
        cx.emit(GitEvent::StashPush(text));
    }

    /// Creates a branch named by the text in the second box.
    pub fn create_branch(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let name = self.detail.read(cx).value().trim().to_owned();
        cx.emit(GitEvent::CreateBranch(name));
    }

    /// Commits with the text in the message box.
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let text = self.message.read(cx).value().to_string();
        cx.emit(GitEvent::Commit(text));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.confirm_push {
                self.answer_push(false, cx);
            } else if self.confirm.is_some() {
                self.answer_discard(false, cx);
            } else {
                cx.emit(GitEvent::Close);
            }
            cx.stop_propagation();
        }
    }
}

fn button(
    theme: Theme,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .border_1()
        .border_color(gpui::rgb(theme.line))
        .hover(move |style| style.bg(gpui::rgb(theme.raised)))
        .child(label.into())
}

impl Render for GitView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let busy = self.busy;
        let heading = match &self.status {
            Some(status) => format!(
                "{} · {}",
                self.project_name,
                status.branch.as_deref().unwrap_or("(ayrık HEAD)")
            ),
            None => self.project_name.clone(),
        };

        let staged: Vec<Entry> = self
            .status
            .iter()
            .flat_map(|status| status.staged().cloned())
            .collect();
        let changes: Vec<Entry> = self
            .status
            .iter()
            .flat_map(|status| status.changes().cloned())
            .collect();

        let staged_rows: Vec<_> = staged
            .iter()
            .cloned()
            .enumerate()
            .map(|(ix, entry)| {
                let letter = entry.staged.letter();
                let path = entry.path.clone();
                let parts = path.clone();
                file_row(theme, ("staged", ix), letter, &entry, cx)
                    .child(button(theme, ("staged-hunks", ix), "Parçalar").on_click(
                        cx.listener(move |this, _, _window, cx| this.open_hunks(&parts, true, cx)),
                    ))
                    .child(
                        button(theme, ("unstage", ix), "Çıkar").on_click(cx.listener(
                            move |this, _, _window, cx| {
                                if !this.busy {
                                    cx.emit(GitEvent::Unstage(vec![path.clone()]));
                                }
                            },
                        )),
                    )
            })
            .collect();
        let change_rows: Vec<_> = changes
            .iter()
            .cloned()
            .enumerate()
            .map(|(ix, entry)| {
                let letter = if entry.untracked {
                    '?'
                } else if entry.conflicted {
                    '!'
                } else {
                    entry.unstaged.letter()
                };
                let (add, discard) = (entry.path.clone(), entry.clone());
                let parts = entry.path.clone();
                let splittable = !entry.untracked && !entry.conflicted;
                file_row(theme, ("change", ix), letter, &entry, cx)
                    .children(splittable.then(|| {
                        button(theme, ("change-hunks", ix), "Parçalar").on_click(cx.listener(
                            move |this, _, _window, cx| this.open_hunks(&parts, false, cx),
                        ))
                    }))
                    .child(button(theme, ("stage", ix), "Ekle").on_click(cx.listener(
                        move |this, _, _window, cx| {
                            if !this.busy {
                                cx.emit(GitEvent::Stage(vec![add.clone()]));
                            }
                        },
                    )))
                    .child(
                        button(theme, ("discard", ix), "Geri al").on_click(cx.listener(
                            move |this, _, _window, cx| this.ask_discard(vec![discard.clone()], cx),
                        )),
                    )
            })
            .collect();

        let branch_buttons: Vec<_> = self
            .branches
            .iter()
            .cloned()
            .enumerate()
            .map(|(ix, name)| {
                let current = ix == 0
                    && self.status.as_ref().and_then(|s| s.branch.as_deref())
                        == Some(name.as_str());
                let target = name.clone();
                let doomed = name.clone();
                let base = button(theme, ("branch", ix), name);
                let base = if current {
                    base.bg(gpui::rgb(theme.selected))
                } else {
                    base
                };
                div()
                    .flex()
                    .flex_row()
                    .child(base.on_click(cx.listener(move |this, _, _window, cx| {
                        if !current && !this.busy {
                            cx.emit(GitEvent::Switch(target.clone()));
                        }
                    })))
                    .children((!current).then(|| {
                        button(theme, ("branch-delete", ix), "×").on_click(cx.listener(
                            move |this, _, _window, cx| {
                                if !this.busy {
                                    cx.emit(GitEvent::DeleteBranch(doomed.clone()));
                                }
                            },
                        ))
                    }))
            })
            .collect();

        let question =
            self.confirm.as_ref().map(|entries| {
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .text_color(gpui::rgb(theme.accent_ink))
                    .child(SharedString::from(format!(
                        "{} dosyadaki değişiklikler kalıcı olarak silinecek.",
                        entries.len()
                    )))
                    .child(button(theme, "discard-yes", "Evet, sil").on_click(
                        cx.listener(|this, _, _window, cx| this.answer_discard(true, cx)),
                    ))
                    .child(button(theme, "discard-no", "Vazgeç").on_click(
                        cx.listener(|this, _, _window, cx| this.answer_discard(false, cx)),
                    ))
            });

        let push_question = self.confirm_push.then(|| {
            div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .text_color(gpui::rgb(theme.accent_ink))
                .child(SharedString::from(format!(
                    "{} dalı uzak depoya gönderilecek.",
                    self.status
                        .as_ref()
                        .and_then(|status| status.branch.as_deref())
                        .unwrap_or("Geçerli")
                )))
                .child(
                    button(theme, "push-yes", "Evet, gönder")
                        .on_click(cx.listener(|this, _, _w, cx| this.answer_push(true, cx))),
                )
                .child(
                    button(theme, "push-no", "Vazgeç")
                        .on_click(cx.listener(|this, _, _w, cx| this.answer_push(false, cx))),
                )
        });
        let stash_rows: Vec<_> =
            self.stashes
                .iter()
                .cloned()
                .map(|stash| {
                    let index = stash.index;
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .px_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(SharedString::from(stash.message)),
                        )
                        .child(button(theme, ("stash-pop", index), "Geri getir").on_click(
                            cx.listener(move |this, _, _w, cx| {
                                if !this.busy {
                                    cx.emit(GitEvent::StashPop(index));
                                }
                            }),
                        ))
                })
                .collect();
        let all_changes = changes.clone();
        let hunk_panel = self.hunks.as_ref().map(|panel| {
            let staged = panel.staged;
            let hunks: Vec<_> = panel
                .diff
                .iter()
                .flat_map(|diff| diff.hunks.iter().cloned().enumerate())
                .map(|(ix, hunk)| {
                    let lines = hunk.lines.into_iter().map(|line| {
                        let colour = match line.chars().next() {
                            Some('+') => 0x3fb950,
                            Some('-') => 0xff7b72,
                            _ => theme.muted,
                        };
                        div()
                            .text_color(gpui::rgb(colour))
                            .child(SharedString::from(line))
                    });
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .mb_2()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_xs()
                                        .text_color(gpui::rgb(theme.accent_ink))
                                        .child(SharedString::from(hunk.header)),
                                )
                                .child(
                                    button(
                                        theme,
                                        ("hunk", ix),
                                        if staged {
                                            "Parçayı çıkar"
                                        } else {
                                            "Parçayı ekle"
                                        },
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _window, cx| this.apply_hunk(ix, cx),
                                    )),
                                ),
                        )
                        .children(lines)
                })
                .collect();
            let empty = panel
                .diff
                .as_ref()
                .is_some_and(|diff| diff.hunks.is_empty());
            div()
                .id("git-hunks")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .items_center()
                        .mb_2()
                        .child(
                            button(theme, "hunks-back", "← Dosyalar")
                                .on_click(cx.listener(|this, _, _window, cx| this.close_hunks(cx))),
                        )
                        .child(SharedString::from(format!(
                            "{} · {}",
                            panel.path,
                            if staged {
                                "hazır parçalar"
                            } else {
                                "parçalar"
                            }
                        ))),
                )
                .children(panel.diff.is_none().then(|| div().child("Okunuyor…")))
                .children(empty.then(|| {
                    div()
                        .text_color(gpui::rgb(theme.muted))
                        .child("Parça yok (ikili, yeni ya da değişmemiş dosya).")
                }))
                .children(hunks)
        });
        let showing_hunks = hunk_panel.is_some();
        div()
            .absolute()
            .occlude()
            .top_8()
            .bottom_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(980.0))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_md()
            .shadow_lg()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_color(gpui::rgb(theme.muted))
                            .child(SharedString::from(format!("GIT · {heading}"))),
                    )
                    .child(button(theme, "git-pull", "Çek").on_click(cx.listener(
                        |this, _, _w, cx| {
                            if !this.busy {
                                cx.emit(GitEvent::Pull);
                            }
                        },
                    )))
                    .child(
                        button(theme, "git-push", "Gönder")
                            .on_click(cx.listener(|this, _, _w, cx| this.ask_push(cx))),
                    )
                    .child(
                        button(theme, "git-stash", "Sakla")
                            .on_click(cx.listener(|this, _, _w, cx| this.stash(cx))),
                    )
                    .child(
                        button(theme, "git-new-branch", "Dal aç")
                            .on_click(cx.listener(|this, _, _w, cx| this.create_branch(cx))),
                    )
                    .child(
                        button(theme, "git-refresh", "Yenile")
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(GitEvent::Refresh))),
                    )
                    .child(
                        button(theme, "git-close", "Kapat (Esc)")
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(GitEvent::Close))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_1()
                    .children(branch_buttons),
            )
            .children(self.error.clone().map(|error| {
                div()
                    .text_color(gpui::rgb(theme.accent_ink))
                    .child(SharedString::from(error))
            }))
            .children(question)
            .children(push_question)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .h(px(32.0))
                            .overflow_hidden()
                            .rounded_sm()
                            .border_1()
                            .border_color(gpui::rgb(theme.line))
                            .child(Input::new(&self.message)),
                    )
                    .child(
                        button(theme, "git-commit", if busy { "…" } else { "Commit" })
                            .on_click(cx.listener(|this, _, _window, cx| this.commit(cx))),
                    ),
            )
            .child(
                div()
                    .h(px(32.0))
                    .overflow_hidden()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.detail)),
            )
            .children(hunk_panel)
            .children((!showing_hunks).then(|| {
                div()
                    .id("git-files")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(section(theme, format!("HAZIR ({})", staged.len())))
                    .children(staged_rows)
                    .child(
                        section(theme, format!("DEĞİŞİKLİKLER ({})", changes.len()))
                            .child(
                                button(theme, "stage-all", "Tümünü ekle").on_click(
                                    cx.listener(|this, _, _window, cx| this.stage_all(cx)),
                                ),
                            )
                            .child(button(theme, "discard-all", "Tümünü geri al").on_click(
                                cx.listener(move |this, _, _window, cx| {
                                    this.ask_discard(all_changes.clone(), cx)
                                }),
                            )),
                    )
                    .children(change_rows)
                    .children(
                        (!stash_rows.is_empty())
                            .then(|| section(theme, format!("SAKLANANLAR ({})", stash_rows.len()))),
                    )
                    .children(stash_rows)
                    .children(
                        (self.status.as_ref().is_some_and(|s| s.entries.is_empty()))
                            .then(|| div().text_color(gpui::rgb(theme.muted)).child("Temiz.")),
                    )
            }))
    }
}

fn section(theme: Theme, title: String) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .gap_2()
        .items_center()
        .mt_2()
        .text_xs()
        .text_color(gpui::rgb(theme.muted))
        .child(div().flex_1().child(SharedString::from(title)))
}

fn file_row(
    theme: Theme,
    id: (&'static str, usize),
    letter: char,
    entry: &Entry,
    cx: &mut Context<GitView>,
) -> gpui::Stateful<gpui::Div> {
    let open = entry.path.clone();
    let shown = match &entry.from {
        Some(from) => format!("{from} → {}", entry.path),
        None => entry.path.clone(),
    };
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded_sm()
        .flex()
        .flex_row()
        .gap_2()
        .items_center()
        .hover(move |style| style.bg(gpui::rgb(theme.raised)))
        .child(
            div()
                .w(px(14.0))
                .text_color(gpui::rgb(theme.accent_ink))
                .child(SharedString::from(letter.to_string())),
        )
        .child(
            div()
                .id((id.0, id.1 + 1_000_000))
                .flex_1()
                .min_w_0()
                .cursor_pointer()
                .child(SharedString::from(shown))
                .on_click(cx.listener(move |_, _, _window, cx| {
                    cx.emit(GitEvent::Open(open.clone()));
                })),
        )
}
