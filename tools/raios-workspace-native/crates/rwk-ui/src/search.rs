//! The project search view: a query over the RAIOS CLI's `locate` results. Read-only; choosing
//! a hit hands it to the workbench, which opens it through the unsaved-edits guard.

use gpui::{
    div, px, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::search::{SearchHit, SearchResult};
use rwk_core::workbench::{Appearance, Theme};

/// What the search view asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    /// The user submitted `pattern`; `ignore_case` is the toggle's state.
    Run(String, bool),
    /// The user chose `path` at `line` (1-based); the workbench opens it through its guard.
    Open(String, usize),
    /// The user closed the view.
    Close,
}

impl EventEmitter<SearchEvent> for SearchView {}

/// What the results area shows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SearchState {
    /// No search ran yet.
    Idle,
    /// A search is running.
    Loading,
    /// The hits of the last search.
    Ready(SearchResult),
    /// Why the last search failed.
    Failed(String),
}

/// The project search view.
pub struct SearchView {
    project_name: String,
    state: SearchState,
    selected: usize,
    ignore_case: bool,
    /// The query of the last submitted search; a different text means Enter runs again instead
    /// of opening the highlighted hit. Comparing values survives the input's own extra
    /// `Change` events (it re-emits one on Enter), which a dirty flag would not.
    last_run: String,
    query: Entity<InputState>,
    theme: Theme,
    _subscription: gpui::Subscription,
}

impl SearchView {
    /// A view for `project_name`; the query box takes focus.
    pub fn new(project_name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        query.update(cx, |input, cx| input.focus(window, cx));
        let subscription =
            cx.subscribe(&query, |this, _input, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    this.selected = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(cx),
                InputEvent::Focus | InputEvent::Blur => {}
            });
        Self {
            project_name,
            state: SearchState::Idle,
            selected: 0,
            ignore_case: false,
            last_run: String::new(),
            query,
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// Repaints the view with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Marks a search as running.
    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.state = SearchState::Loading;
        self.selected = 0;
        cx.notify();
    }

    /// Shows the hits of the last search.
    pub fn set_result(&mut self, result: SearchResult, cx: &mut Context<Self>) {
        self.state = SearchState::Ready(result);
        self.selected = 0;
        cx.notify();
    }

    /// Shows why the last search failed.
    pub fn set_failure(&mut self, message: String, cx: &mut Context<Self>) {
        self.state = SearchState::Failed(message);
        cx.notify();
    }

    /// The hits on screen, when the last search answered.
    pub fn result(&self) -> Option<&SearchResult> {
        match &self.state {
            SearchState::Ready(result) => Some(result),
            _ => None,
        }
    }

    /// The failure message on screen, when the last search failed.
    pub fn failure(&self) -> Option<&str> {
        match &self.state {
            SearchState::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// Index of the highlighted hit.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Whether the case toggle is on.
    pub fn ignore_case(&self) -> bool {
        self.ignore_case
    }

    /// Replaces the query text (as typing would).
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.selected = 0;
        cx.notify();
    }

    /// Chooses the highlighted hit, when there is one.
    pub fn choose_selected(&mut self, cx: &mut Context<Self>) {
        if let SearchState::Ready(result) = &self.state {
            if let Some(hit) = result.hits.get(self.selected) {
                cx.emit(SearchEvent::Open(hit.path.clone(), hit.line));
            }
        }
    }

    /// Flips the case toggle; a query that already ran is sent again with the new flag.
    pub fn toggle_case(&mut self, cx: &mut Context<Self>) {
        self.ignore_case = !self.ignore_case;
        if !matches!(self.state, SearchState::Idle) {
            self.submit(cx);
        }
        cx.notify();
    }

    /// Runs the query currently in the box.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let pattern = self.query.read(cx).value().to_string();
        self.last_run = pattern.clone();
        cx.emit(SearchEvent::Run(pattern, self.ignore_case));
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SearchEvent::Close),
            "down" => self.selected = self.selected.saturating_add(1),
            "up" => self.selected = self.selected.saturating_sub(1),
            "enter" => {
                // The first Enter runs the query (the input turns it into a submit); once the
                // results of that exact query are on screen, Enter opens the highlighted hit.
                let text = self.query.read(cx).value().to_string();
                let empty = self.result().is_none_or(|result| result.hits.is_empty());
                if text != self.last_run || empty {
                    return;
                }
                self.choose_selected(cx);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

fn button(
    theme: Theme,
    id: &'static str,
    label: impl Into<SharedString>,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let base = div()
        .id(id)
        .px_3()
        .py_0p5()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |style| style.bg(gpui::rgb(theme.raised)))
        .border_1()
        .border_color(gpui::rgb(if active {
            theme.accent_ink
        } else {
            theme.line
        }))
        .child(label.into());
    if active {
        base.bg(gpui::rgb(theme.selected))
    } else {
        base
    }
}

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let family = crate::fonts::monospace_family(window);
        let theme = self.theme;
        let (status, message) = match &self.state {
            SearchState::Idle => (
                format!("{} · hazır", self.project_name),
                Some("Aramak için bir desen yazıp Enter'a basın.".to_owned()),
            ),
            SearchState::Loading => (format!("{} · aranıyor…", self.project_name), None),
            SearchState::Ready(result) => {
                let mut status = format!("{} · {} eşleşme", self.project_name, result.hits.len());
                if result.truncated {
                    status.push_str(" (ilk sonuçlar gösteriliyor)");
                }
                let message = result.hits.is_empty().then(|| "Eşleşme yok.".to_owned());
                (status, message)
            }
            SearchState::Failed(error) => (
                format!("{} · arama başarısız", self.project_name),
                Some(error.clone()),
            ),
        };
        let hits: Vec<SearchHit> = match &self.state {
            SearchState::Ready(result) => result.hits.clone(),
            _ => Vec::new(),
        };
        let selected = self.selected.min(hits.len().saturating_sub(1));
        let rows = hits.into_iter().enumerate().map(|(ix, hit)| {
            let row = div()
                .id(("search-hit", ix))
                .px_3()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(gpui::rgb(theme.muted))
                        .child(SharedString::from(format!("{}:{}", hit.path, hit.line))),
                )
                .child(
                    div()
                        .font_family(family)
                        .child(SharedString::from(hit.text)),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.selected = ix;
                    cx.emit(SearchEvent::Open(hit.path.clone(), hit.line));
                }));
            if ix == selected {
                row.bg(gpui::rgb(theme.selected))
            } else {
                row
            }
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
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .text_color(gpui::rgb(theme.muted))
                            .child("PROJEDE ARA"),
                    )
                    .child(
                        div()
                            .text_color(gpui::rgb(theme.muted))
                            .child(SharedString::from(self.project_name.clone())),
                    )
                    .child(
                        button(
                            theme,
                            "search-case",
                            if self.ignore_case {
                                "Aa: yok sayılıyor"
                            } else {
                                "Aa: duyarlı"
                            },
                            self.ignore_case,
                        )
                        .on_click(cx.listener(|this, _, _window, cx| this.toggle_case(cx))),
                    )
                    .child(
                        button(theme, "search-close", "Kapat (Esc)", false)
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(SearchEvent::Close))),
                    ),
            )
            .child(
                div()
                    .h(px(32.0))
                    .overflow_hidden()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.query)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(SharedString::from(status)),
            )
            .child(
                div()
                    .id("search-results")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .children(message.map(|message| {
                        div()
                            .text_color(gpui::rgb(theme.accent_ink))
                            .child(SharedString::from(message))
                    }))
                    .children(rows),
            )
    }
}
