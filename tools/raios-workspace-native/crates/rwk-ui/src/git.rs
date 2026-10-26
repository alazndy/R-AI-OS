//! The source control view: the repository's changed files, staging, the commit message and the
//! branch list. It only shows state and asks; the workbench runs git and sends the new status.

use gpui::{
    div, px, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::git::{Entry, Status};
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
    /// Close the view.
    Close,
}

impl EventEmitter<GitEvent> for GitView {}

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
    message: Entity<InputState>,
    theme: Theme,
    _subscription: Subscription,
}

impl GitView {
    /// A view for `project_name`; the message box takes focus.
    pub fn new(project_name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let message = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
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
            message,
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
        cx.notify();
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
            if self.confirm.is_some() {
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
                file_row(theme, ("staged", ix), letter, &entry, cx).child(
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
                file_row(theme, ("change", ix), letter, &entry, cx)
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
                let base = button(theme, ("branch", ix), name);
                let base = if current {
                    base.bg(gpui::rgb(theme.selected))
                } else {
                    base
                };
                base.on_click(cx.listener(move |this, _, _window, cx| {
                    if !current && !this.busy {
                        cx.emit(GitEvent::Switch(target.clone()));
                    }
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

        let all_changes = changes.clone();
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
                        (self.status.as_ref().is_some_and(|s| s.entries.is_empty()))
                            .then(|| div().text_color(gpui::rgb(theme.muted)).child("Temiz.")),
                    ),
            )
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
