//! The terminal rename prompt: a small modal with one text box. It only validates and reports;
//! the workbench applies the name to the pane and saves the workspace.

use gpui::{
    div, Context, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
};
use gpui::{AppContext, Entity};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::sessions::SessionName;
use rwk_core::workbench::{Appearance, Theme};

/// What the prompt is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptPurpose {
    /// Name a terminal.
    RenameTerminal,
    /// Title a new operator task.
    NewTask,
    /// Name a new file of the project.
    NewFile,
    /// Name a new folder of the project.
    NewFolder,
    /// Rename a file or folder of the project.
    RenameEntry,
    /// Rename the symbol under the editor's cursor.
    RenameSymbol,
    /// Ask for a line (and column) to go to.
    GoToLine,
}

/// What the prompt asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenamePromptEvent {
    /// The user committed a task title that passed validation.
    NewTask(String),
    /// The user committed a file or folder name that passed validation.
    EntryName(String),
    /// The user committed a name, or an empty text, which removes the name.
    Rename(Option<SessionName>),
    /// The user closed the prompt.
    Close,
}

impl EventEmitter<RenamePromptEvent> for RenameTerminalPrompt {}

/// The rename prompt for one terminal.
pub struct RenameTerminalPrompt {
    input: Entity<InputState>,
    problem: Option<SharedString>,
    purpose: PromptPurpose,
    running_agent: bool,
    theme: Theme,
    _subscription: Subscription,
}

impl RenameTerminalPrompt {
    /// A prompt that starts with the terminal's current name; the text box takes focus.
    /// `running_agent` adds a note that the daemon keeps the name the run started with.
    pub fn new(
        current: Option<&SessionName>,
        running_agent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        if let Some(current) = current {
            input.update(cx, |input, cx| {
                input.set_value(current.as_str(), window, cx)
            });
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe(&input, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit(cx);
            }
        });
        Self {
            input,
            problem: None,
            purpose: PromptPurpose::RenameTerminal,
            running_agent,
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// A prompt for the title of a new task.
    pub fn new_task(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut prompt = Self::new(None, false, window, cx);
        prompt.purpose = PromptPurpose::NewTask;
        prompt
    }

    /// A prompt for the name of a file or folder, starting with `initial`.
    pub fn entry(
        purpose: PromptPurpose,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut prompt = Self::new(None, false, window, cx);
        prompt.purpose = purpose;
        if !initial.is_empty() {
            prompt.set_text(initial, window, cx);
        }
        prompt
    }

    /// Repaints the prompt with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// The text typed so far.
    pub fn text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Replaces the text (as typing would).
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// The message shown under the text box.
    pub fn problem(&self) -> Option<String> {
        self.problem.as_ref().map(ToString::to_string)
    }

    /// Validates the text and, when it passes, asks the workbench to apply it. An empty text
    /// removes the name.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if self.purpose == PromptPurpose::NewTask {
            match rwk_core::tasks::validate_title(&text) {
                Ok(title) => {
                    self.problem = None;
                    cx.emit(RenamePromptEvent::NewTask(title));
                }
                Err(error) => self.problem = Some(error.message.into()),
            }
            cx.notify();
            return;
        }
        if self.purpose == PromptPurpose::GoToLine {
            let target = text.trim();
            if rwk_core::workbench::parse_line_target(target).is_some() {
                self.problem = None;
                cx.emit(RenamePromptEvent::EntryName(target.to_owned()));
            } else {
                self.problem = Some("Satır numarası yazın (ör. 120 ya da 120:8)".into());
            }
            cx.notify();
            return;
        }
        if self.purpose == PromptPurpose::RenameSymbol {
            let name = text.trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                self.problem = Some("Boşluksuz bir ad yazın".into());
            } else {
                self.problem = None;
                cx.emit(RenamePromptEvent::EntryName(name.to_owned()));
            }
            cx.notify();
            return;
        }
        if matches!(
            self.purpose,
            PromptPurpose::NewFile | PromptPurpose::NewFolder | PromptPurpose::RenameEntry
        ) {
            let name = text.trim();
            match rwk_core::project::validate_entry_name(name) {
                Ok(()) => {
                    self.problem = None;
                    cx.emit(RenamePromptEvent::EntryName(name.to_owned()));
                }
                Err(error) => self.problem = Some(error.message.into()),
            }
            cx.notify();
            return;
        }
        if text.trim().is_empty() {
            self.problem = None;
            cx.emit(RenamePromptEvent::Rename(None));
        } else {
            match SessionName::parse(&text) {
                Ok(name) => {
                    self.problem = None;
                    cx.emit(RenamePromptEvent::Rename(Some(name)));
                }
                Err(error) => self.problem = Some(error.message.into()),
            }
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(RenamePromptEvent::Close);
            cx.stop_propagation();
        }
    }
}

impl Render for RenameTerminalPrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let heading = match self.purpose {
            PromptPurpose::RenameTerminal => "Terminali yeniden adlandır",
            PromptPurpose::NewTask => "Yeni görev",
            PromptPurpose::NewFile => "Yeni dosya",
            PromptPurpose::NewFolder => "Yeni klasör",
            PromptPurpose::RenameEntry => "Yeniden adlandır",
            PromptPurpose::RenameSymbol => "Sembolü yeniden adlandır",
            PromptPurpose::GoToLine => "Satıra git",
        };
        let note = if self.purpose == PromptPurpose::GoToLine {
            "Satır numarası, ya da satır:sütun. Enter gider, Esc kapatır."
        } else if self.purpose == PromptPurpose::RenameSymbol {
            "Ad, dil sunucusunun bulduğu her kullanımda değişir; açık sekmeler kaydedilmeden güncellenir. Enter uygular, Esc kapatır."
        } else if matches!(
            self.purpose,
            PromptPurpose::NewFile | PromptPurpose::NewFolder | PromptPurpose::RenameEntry
        ) {
            "Tek bir ad yazın ('/' olmadan); var olan bir kayıt değiştirilmez. Enter kaydeder, Esc kapatır."
        } else if self.purpose == PromptPurpose::NewTask {
            "Görev açık projeye eklenir; başlık denetim kaydına geçer, parola ve anahtar yazmayın. Enter ekler, Esc kapatır."
        } else if self.running_agent {
            "Çalışan ajanın daemon kaydındaki adı başlatıldığı adla kalır; bu ad yalnızca burada görünür."
        } else {
            "Boş bırakırsanız ad kaldırılır. Enter kaydeder, Esc kapatır."
        };
        div()
            .id("rename-terminal-prompt")
            .absolute()
            .occlude()
            .child(
                crate::overlay::close_button(self.theme, "rename-close")
                    .on_click(cx.listener(|_, _, _window, cx| cx.emit(RenamePromptEvent::Close))),
            )
            .top_16()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(520.0))
            .p_4()
            .rounded_md()
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_2()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(SharedString::from(heading))
            .child(
                div()
                    .h(gpui::px(30.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.input)),
            )
            .children(self.problem.clone().map(|problem| {
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.bad))
                    .child(problem)
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(note),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("rename-terminal-confirm")
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(gpui::rgb(theme.accent_ink))
                            .child(SharedString::from("Kaydet"))
                            .on_click(cx.listener(|this, _, _window, cx| this.submit(cx))),
                    )
                    .child(
                        div()
                            .id("rename-terminal-cancel")
                            .px_3()
                            .py_1()
                            .border_1()
                            .border_color(gpui::rgb(theme.line))
                            .child(SharedString::from("Vazgeç"))
                            .on_click(cx.listener(|_, _, _window, cx| {
                                cx.emit(RenamePromptEvent::Close);
                            })),
                    ),
            )
    }
}
