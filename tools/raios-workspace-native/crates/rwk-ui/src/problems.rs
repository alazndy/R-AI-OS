//! The Problems view: what the language servers found, errors first. Choosing a line hands the
//! file and line to the workbench, which opens it through the unsaved-edits guard.

use gpui::{
    div, px, Context, EventEmitter, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use rwk_core::lsp::{Problem, Severity};
use rwk_core::workbench::{Appearance, Theme};

/// What the Problems view asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemsEvent {
    /// The user chose `path` at `line` (1-based).
    Open(String, usize),
    /// The user closed the view.
    Close,
}

impl EventEmitter<ProblemsEvent> for ProblemsView {}

/// The Problems view.
pub struct ProblemsView {
    title: SharedString,
    /// The servers' list, errors first.
    raw: Vec<Problem>,
    /// What is on screen, in order: `raw`, or by file when `grouped`.
    problems: Vec<Problem>,
    grouped: bool,
    selected: usize,
    theme: Theme,
    focus: FocusHandle,
}

impl ProblemsView {
    /// A view of `problems`; it takes focus so the arrow keys and Esc work.
    pub fn new(problems: Vec<Problem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::titled("SORUNLAR", problems, window, cx)
    }

    /// A list under another heading (usages, a file's symbols); it shows what it was given
    /// and is not touched by the language servers' later diagnostics.
    pub fn titled(
        title: impl Into<SharedString>,
        problems: Vec<Problem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            title: title.into(),
            raw: problems.clone(),
            problems,
            grouped: false,
            selected: 0,
            theme: Theme::of(Appearance::default()),
            focus,
        }
    }

    /// Repaints the view with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Replaces the list (the servers sent new diagnostics while the view is open).
    pub fn set_problems(&mut self, problems: Vec<Problem>, cx: &mut Context<Self>) {
        self.raw = problems;
        self.arrange();
        self.selected = self.selected.min(self.problems.len().saturating_sub(1));
        cx.notify();
    }

    /// Puts the list in the order on screen.
    fn arrange(&mut self) {
        self.problems = self.raw.clone();
        if self.grouped {
            self.problems
                .sort_by(|a, b| (&a.path, a.line, a.column).cmp(&(&b.path, b.line, b.column)));
        }
    }

    /// Switches between the severity order and one section per file.
    pub fn toggle_grouping(&mut self, cx: &mut Context<Self>) {
        self.grouped = !self.grouped;
        self.arrange();
        self.selected = 0;
        cx.notify();
    }

    /// Whether the list is grouped by file.
    pub fn grouped(&self) -> bool {
        self.grouped
    }

    /// The lines on screen.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// Index of the highlighted line.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the highlight by `delta`, staying inside the list.
    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let last = self.problems.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        cx.notify();
    }

    /// Chooses the highlighted line, when there is one.
    pub fn choose_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(problem) = self.problems.get(self.selected) {
            cx.emit(ProblemsEvent::Open(problem.path.clone(), problem.line));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(ProblemsEvent::Close),
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            "enter" => self.choose_selected(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn marker(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "✖",
        Severity::Warning => "▲",
        Severity::Info => "●",
        Severity::Hint => "○",
    }
}

impl Render for ProblemsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let selected = self.selected;
        let status = if self.problems.is_empty() {
            "Sonuç yok.".to_owned()
        } else {
            format!("{} satır", self.problems.len())
        };
        let grouped = self.grouped;
        let counts: std::collections::HashMap<String, (usize, usize)> =
            self.problems
                .iter()
                .fold(std::collections::HashMap::new(), |mut map, problem| {
                    let entry: &mut (usize, usize) = map.entry(problem.path.clone()).or_default();
                    match problem.severity {
                        Severity::Error => entry.0 += 1,
                        _ => entry.1 += 1,
                    }
                    map
                });
        let mut previous: Option<String> = None;
        let rows = self
            .problems
            .iter()
            .cloned()
            .enumerate()
            .map(|(ix, problem)| {
                let header =
                    (grouped && previous.as_deref() != Some(problem.path.as_str())).then(|| {
                        let (errors, others) =
                            counts.get(&problem.path).copied().unwrap_or_default();
                        div()
                            .mt_2()
                            .px_3()
                            .text_xs()
                            .text_color(gpui::rgb(theme.muted))
                            .child(SharedString::from(format!(
                                "{} · {errors} hata, {others} diğer",
                                problem.path
                            )))
                    });
                previous = Some(problem.path.clone());
                let color = match problem.severity {
                    Severity::Error => theme.accent_ink,
                    _ => theme.muted,
                };
                let source = problem
                    .source
                    .as_ref()
                    .map(|source| format!(" ({source})"))
                    .unwrap_or_default();
                let row = div()
                    .id(("problem", ix))
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .text_color(gpui::rgb(color))
                            .child(marker(problem.severity)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(SharedString::from(format!("{}{source}", problem.message))),
                    )
                    .child(div().text_xs().text_color(gpui::rgb(theme.muted)).child(
                        SharedString::from(format!(
                            "{}:{}:{}",
                            problem.path, problem.line, problem.column
                        )),
                    ))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.selected = ix;
                        this.choose_selected(cx);
                    }));
                let row = if ix == selected {
                    row.bg(gpui::rgb(theme.selected))
                } else {
                    row
                };
                div().flex().flex_col().children(header).child(row)
            });
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
            .track_focus(&self.focus)
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
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .id("problems-group")
                            .px_3()
                            .py_0p5()
                            .rounded_sm()
                            .cursor_pointer()
                            .border_1()
                            .border_color(gpui::rgb(if grouped {
                                theme.accent_ink
                            } else {
                                theme.line
                            }))
                            .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                            .child("Dosyaya göre")
                            .on_click(cx.listener(|this, _, _window, cx| this.toggle_grouping(cx))),
                    )
                    .child(
                        div()
                            .id("problems-close")
                            .px_3()
                            .py_0p5()
                            .rounded_sm()
                            .cursor_pointer()
                            .border_1()
                            .border_color(gpui::rgb(theme.line))
                            .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                            .child("Kapat (Esc)")
                            .on_click(
                                cx.listener(|_, _, _window, cx| cx.emit(ProblemsEvent::Close)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(SharedString::from(status)),
            )
            .child(
                div()
                    .id("problems-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(rows),
            )
    }
}
