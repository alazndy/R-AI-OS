//! Shared command and quick-open palette: a query box over commands or project files.

use gpui::{
    div, App, AppContext, Context, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent,
    ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window,
};
use gpui_base::input::{InputEvent, InputState};
use gpui_base::Input;
use rwk_core::workbench::{palette_entries, CommandId, PaletteEntry, WorkbenchState};

/// What the palette tells the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEvent {
    /// Run this command (it was enabled when chosen).
    Run(CommandId),
    /// Open a project-relative file through the workbench decision guard.
    OpenFile(String),
    /// Close without running anything.
    Close,
}

/// The open palette.
pub struct PaletteView {
    input: Entity<InputState>,
    state: WorkbenchState,
    entries: Vec<PaletteEntry>,
    selected: usize,
    scroll: ScrollHandle,
    files: Option<Vec<String>>,
    filtered_files: Vec<String>,
    loading: bool,
    truncated: bool,
    _subscription: Subscription,
}

use gpui::Entity;
use rwk_core::workbench::quick_open::{matching_files, FileListing};

impl EventEmitter<PaletteEvent> for PaletteView {}

impl PaletteView {
    /// A palette over the commands available in `state`; the query box takes focus.
    pub fn new(state: WorkbenchState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The state is multi-line by construction; without `submit_on_enter` a real Enter key would
        // insert a newline into the query before it confirms.
        let input = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription =
            cx.subscribe(&input, |this, _input, event: &InputEvent, cx| match event {
                InputEvent::Change => this.refilter(cx),
                InputEvent::PressEnter { .. } => this.confirm(cx),
                InputEvent::Focus | InputEvent::Blur => {}
            });
        Self {
            input,
            state,
            entries: palette_entries("", &state),
            selected: 0,
            scroll: ScrollHandle::new(),
            files: None,
            filtered_files: Vec::new(),
            loading: false,
            truncated: false,
            _subscription: subscription,
        }
    }

    /// A filename picker using the existing palette input and keyboard navigation.
    pub fn for_files(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new(WorkbenchState::default(), window, cx);
        view.files = Some(Vec::new());
        view.entries.clear();
        view.loading = true;
        view
    }

    /// True when this palette is selecting a file rather than a command.
    pub fn is_file_picker(&self) -> bool {
        self.files.is_some()
    }

    /// Replace the discovered files while keeping any query typed during discovery.
    pub fn set_files(&mut self, listing: FileListing, cx: &mut Context<Self>) {
        self.files = Some(listing.files);
        self.loading = false;
        self.truncated = listing.truncated;
        self.refilter(cx);
    }

    /// The project-relative file paths currently matching the filename query.
    pub fn file_paths(&self) -> &[String] {
        &self.filtered_files
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        if let Some(files) = &self.files {
            self.filtered_files = matching_files(files, &query, 100);
        } else {
            self.entries = palette_entries(&query, &self.state);
        }
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    /// The text in the query box.
    pub fn query(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Replaces the query (as typing would).
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.refilter(cx);
    }

    /// The rows currently shown.
    pub fn entries(&self) -> &[PaletteEntry] {
        &self.entries
    }

    /// Index of the highlighted row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Moves the highlight, wrapping at both ends.
    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = if self.is_file_picker() {
            self.filtered_files.len()
        } else {
            self.entries.len()
        };
        if count == 0 {
            return;
        }
        let count = count as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    /// Activates the highlighted file or available command. An unavailable command stays
    /// on screen with its reason.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.is_file_picker() {
            if let Some(path) = self.filtered_files.get(self.selected) {
                cx.emit(PaletteEvent::OpenFile(path.clone()));
            }
            return;
        }
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.state.enabled {
                cx.emit(PaletteEvent::Run(entry.id));
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(PaletteEvent::Close),
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// Height of the filter box; the input would otherwise take all the room it is given.
const INPUT_HEIGHT: f32 = 32.0;

impl Render for PaletteView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let marker = if index == self.selected { "› " } else { "  " };
                let shortcut = entry.shortcut.unwrap_or("");
                let reason = entry.state.reason.unwrap_or("");
                let text = if entry.state.enabled {
                    format!("{marker}{}   {shortcut}", entry.label)
                } else {
                    format!("{marker}{}   ({reason})", entry.label)
                };
                let row = div().px_2().child(SharedString::from(text));
                if entry.state.enabled {
                    row
                } else {
                    row.opacity(0.5)
                }
            })
            .collect();
        for (index, path) in self.filtered_files.iter().enumerate() {
            let marker = if index == self.selected { "› " } else { "  " };
            let chosen = path.clone();
            rows.push(
                div()
                    .px_2()
                    .child(SharedString::from(format!("{marker}{path}")))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |_, _, _, cx| {
                            cx.emit(PaletteEvent::OpenFile(chosen.clone()));
                        }),
                    ),
            );
        }
        let hint = if self.loading {
            "Dosyalar aranıyor…"
        } else if self.truncated {
            "İlk 10.000 girdide arama; daha dar bir proje açın"
        } else if self.is_file_picker() && self.filtered_files.is_empty() {
            "Dosya bulunamadı"
        } else {
            ""
        };
        div()
            .absolute()
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(560.0))
            .p_2()
            .bg(gpui::rgb(0x252526))
            .text_color(gpui::rgb(0xe5e5e5))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                div()
                    .h(gpui::px(INPUT_HEIGHT))
                    .overflow_hidden()
                    .child(Input::new(&self.input)),
            )
            .child(hint)
            .child(
                div()
                    .id("palette-results")
                    .max_h(gpui::px(360.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
    }
}
