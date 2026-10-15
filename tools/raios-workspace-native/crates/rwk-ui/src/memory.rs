//! The memory view: the project's `memory.md` as a heading outline with checklist progress, and
//! the structured RAIOS records. Read-only. What is shown is inert text; links, images and HTML
//! of the source are never interpreted.

use gpui::{
    div, px, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_base::input::{Input, InputEvent, InputState};
use rwk_core::memory::MemorySnapshot;
use rwk_core::memory_model::{
    blocks, filter_records, filter_sections, layer_name, sections, Block,
};
use rwk_core::workbench::{Appearance, Theme};

/// What the memory view asks of the workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryEvent {
    /// The user closed the view.
    Close,
    /// The user asked for the sources to be read again.
    Refresh,
}

impl EventEmitter<MemoryEvent> for MemoryView {}

/// Which source is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySourceKind {
    /// The project's `memory.md`.
    Document,
    /// The structured records in `workspace.db`.
    Records,
}

/// What a source delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded<T> {
    /// Still being read.
    Loading,
    /// Read.
    Ready(T),
    /// Failed, with a message for the user.
    Failed(String),
}

/// The memory view.
pub struct MemoryView {
    project_name: String,
    document: Loaded<Option<String>>,
    records: Loaded<MemorySnapshot>,
    source: MemorySourceKind,
    selected: usize,
    raw: bool,
    kind_filter: Option<String>,
    layer_filter: Option<i64>,
    query: Entity<InputState>,
    theme: Theme,
    _subscription: gpui::Subscription,
}

impl MemoryView {
    /// A view for `project_name`; both sources start out loading.
    pub fn new(project_name: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        query.update(cx, |input, cx| input.focus(window, cx));
        let subscription = cx.subscribe(&query, |this, _input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.selected = 0;
                cx.notify();
            }
        });
        Self {
            project_name,
            document: Loaded::Loading,
            records: Loaded::Loading,
            source: MemorySourceKind::Document,
            selected: 0,
            raw: false,
            kind_filter: None,
            layer_filter: None,
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

    /// Sets what `memory.md` delivered (`None` content means the file does not exist).
    pub fn set_document(&mut self, document: Loaded<Option<String>>, cx: &mut Context<Self>) {
        self.document = document;
        self.selected = 0;
        cx.notify();
    }

    /// Sets what the record source delivered.
    pub fn set_records(&mut self, records: Loaded<MemorySnapshot>, cx: &mut Context<Self>) {
        self.records = records;
        self.selected = 0;
        cx.notify();
    }

    /// Marks both sources as loading again.
    pub fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.document = Loaded::Loading;
        self.records = Loaded::Loading;
        cx.notify();
    }

    /// Switches the source.
    pub fn show(&mut self, source: MemorySourceKind, cx: &mut Context<Self>) {
        self.source = source;
        self.selected = 0;
        cx.notify();
    }

    /// The source being shown.
    pub fn source(&self) -> MemorySourceKind {
        self.source
    }

    /// Replaces the search text (as typing would).
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.query
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.selected = 0;
        cx.notify();
    }

    /// The titles of the outline entries currently listed.
    pub fn outline(&self, cx: &gpui::App) -> Vec<String> {
        let query = self.query.read(cx).value().to_string();
        match self.source {
            MemorySourceKind::Document => match &self.document {
                Loaded::Ready(Some(text)) => filter_sections(sections(text), &query)
                    .into_iter()
                    .map(|section| section.title)
                    .collect(),
                _ => Vec::new(),
            },
            MemorySourceKind::Records => match &self.records {
                Loaded::Ready(snapshot) => filter_records(
                    &snapshot.items,
                    &query,
                    self.kind_filter.as_deref(),
                    self.layer_filter,
                )
                .into_iter()
                .map(|item| item.title.clone())
                .collect(),
                _ => Vec::new(),
            },
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(MemoryEvent::Close),
            "down" => self.selected = self.selected.saturating_add(1),
            "up" => self.selected = self.selected.saturating_sub(1),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn cycle_kind(&mut self, snapshot: &MemorySnapshot, cx: &mut Context<Self>) {
        let mut kinds: Vec<String> = snapshot.items.iter().map(|i| i.item_type.clone()).collect();
        kinds.sort();
        kinds.dedup();
        self.kind_filter = match &self.kind_filter {
            None => kinds.first().cloned(),
            Some(current) => kinds
                .iter()
                .position(|kind| kind == current)
                .and_then(|ix| kinds.get(ix + 1).cloned()),
        };
        self.selected = 0;
        cx.notify();
    }

    fn cycle_layer(&mut self, cx: &mut Context<Self>) {
        self.layer_filter = match self.layer_filter {
            None => Some(1),
            Some(3) => None,
            Some(layer) => Some(layer + 1),
        };
        self.selected = 0;
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

fn draw(theme: Theme, blocks: Vec<Block>, family: &'static str) -> impl IntoElement {
    // `min_w_0` lets the column shrink to its parent so long lines wrap instead of widening it.
    let rows = blocks.into_iter().map(|block| match block {
        Block::Heading { level, text } => div()
            .pt_2()
            .text_color(gpui::rgb(theme.blue_ink))
            .text_size(px(20.0 - (level.min(4) as f32) * 1.5))
            .child(SharedString::from(text))
            .into_any_element(),
        Block::Paragraph(text) => div()
            .min_w_0()
            .child(SharedString::from(text))
            .into_any_element(),
        Block::Item {
            depth,
            checked,
            text,
        } => {
            let mark = match checked {
                Some(true) => "☑ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            div()
                .min_w_0()
                .pl(px(12.0 + 16.0 * depth as f32))
                .child(SharedString::from(format!("{mark}{text}")))
                .into_any_element()
        }
        Block::Code(line) => div()
            .pl_3()
            .font_family(family)
            .text_color(gpui::rgb(theme.accent_ink))
            .child(SharedString::from(line))
            .into_any_element(),
        Block::Row(cells) => div()
            .flex()
            .flex_row()
            .gap_3()
            .children(
                cells
                    .into_iter()
                    .map(|cell| div().min_w(px(80.0)).child(SharedString::from(cell))),
            )
            .into_any_element(),
        Block::Truncated => div()
            .text_color(gpui::rgb(theme.accent_ink))
            .child("Bölüm 2.000 satırda kesildi; tamamı için dosyada aç.")
            .into_any_element(),
    });
    div().min_w_0().flex().flex_col().gap_0p5().children(rows)
}

impl Render for MemoryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let family = crate::fonts::monospace_family(window);
        let theme = self.theme;
        let query = self.query.read(cx).value().to_string();
        let mut entries: Vec<(SharedString, Option<SharedString>)> = Vec::new();
        let mut detail: Vec<Block> = Vec::new();
        let mut status = SharedString::from(format!("{} · salt okunur", self.project_name));
        let mut message: Option<String> = None;
        let mut progress: Option<(usize, usize)> = None;
        let mut snapshot_for_filter: Option<MemorySnapshot> = None;
        match self.source {
            MemorySourceKind::Document => match &self.document {
                Loaded::Loading => message = Some("memory.md yükleniyor…".into()),
                Loaded::Failed(error) => message = Some(error.clone()),
                Loaded::Ready(None) => message = Some("memory.md bulunamadı.".into()),
                Loaded::Ready(Some(text)) if text.trim().is_empty() => {
                    message = Some("memory.md boş.".into());
                }
                Loaded::Ready(Some(text)) => {
                    let found = filter_sections(sections(text), &query);
                    if found.is_empty() {
                        message = Some("Aramayla eşleşen bölüm yok.".into());
                    }
                    let current = self.selected.min(found.len().saturating_sub(1));
                    status = SharedString::from(format!("{status} · {} bölüm", found.len()));
                    for section in &found {
                        let badge = (section.objectives > 0).then(|| {
                            SharedString::from(format!(
                                "{}/{}",
                                section.completed, section.objectives
                            ))
                        });
                        let indent = "  ".repeat(section.level.saturating_sub(1).min(3));
                        entries.push((
                            SharedString::from(format!("{indent}{}", section.title)),
                            badge,
                        ));
                    }
                    if let Some(section) = found.get(current) {
                        if section.objectives > 0 {
                            progress = Some((section.completed, section.objectives));
                        }
                        detail = if self.raw {
                            section
                                .content
                                .split('\n')
                                .map(|l| Block::Code(l.to_owned()))
                                .collect()
                        } else {
                            blocks(&section.content)
                        };
                    }
                }
            },
            MemorySourceKind::Records => match &self.records {
                Loaded::Loading => message = Some("Kayıtlar yükleniyor…".into()),
                Loaded::Failed(error) => message = Some(error.clone()),
                Loaded::Ready(snapshot) => {
                    snapshot_for_filter = Some(snapshot.clone());
                    let found = filter_records(
                        &snapshot.items,
                        &query,
                        self.kind_filter.as_deref(),
                        self.layer_filter,
                    );
                    status = SharedString::from(format!(
                        "{status} · {}/{} kayıt · L1 olgular · L2 sahneler · L3 persona",
                        found.len(),
                        snapshot.total
                    ));
                    if found.is_empty() {
                        message = Some("Eşleşen kayıt yok.".into());
                    }
                    for item in &found {
                        entries.push((
                            SharedString::from(item.title.clone()),
                            Some(SharedString::from(format!(
                                "{} · {}",
                                item.item_type,
                                layer_name(item.layer)
                            ))),
                        ));
                    }
                    let current = self.selected.min(found.len().saturating_sub(1));
                    if let Some(item) = found.get(current) {
                        detail.push(Block::Heading {
                            level: 2,
                            text: item.title.clone(),
                        });
                        detail.push(Block::Paragraph(item.description.clone()));
                        detail.extend(blocks(&item.body));
                        if item.truncated {
                            detail.push(Block::Paragraph(
                                "(gövde yüklenen sınırda kesildi)".to_owned(),
                            ));
                        }
                    }
                }
            },
        }
        let selected = self.selected.min(entries.len().saturating_sub(1));
        let outline = entries.into_iter().enumerate().map(|(ix, (title, badge))| {
            let row = div()
                .id(("memory-entry", ix))
                .px_2()
                .py_0p5()
                .flex()
                .flex_col()
                .child(title)
                .children(
                    badge.map(|b| div().text_xs().text_color(gpui::rgb(theme.muted)).child(b)),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.selected = ix;
                    cx.notify();
                }));
            if ix == selected {
                row.bg(gpui::rgb(theme.selected))
            } else {
                row
            }
        });
        let document_mode = self.source == MemorySourceKind::Document;
        let kind_label = SharedString::from(format!(
            "Tür: {}",
            self.kind_filter.as_deref().unwrap_or("tümü")
        ));
        let layer_label = SharedString::from(format!(
            "Katman: {}",
            self.layer_filter.map_or("tümü", layer_name)
        ));
        div()
            .absolute()
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
                    .child(div().text_color(gpui::rgb(theme.muted)).child("HAFIZA"))
                    .child(
                        button(theme, "memory-source-document", "memory.md", document_mode)
                            .on_click(cx.listener(|this, _, _w, cx| {
                                this.show(MemorySourceKind::Document, cx)
                            })),
                    )
                    .child(
                        button(
                            theme,
                            "memory-source-records",
                            "RAIOS kayıtları",
                            !document_mode,
                        )
                        .on_click(
                            cx.listener(|this, _, _w, cx| this.show(MemorySourceKind::Records, cx)),
                        ),
                    )
                    .children(document_mode.then(|| {
                        button(
                            theme,
                            "memory-raw",
                            if self.raw {
                                "Görsel görünüm"
                            } else {
                                "Ham metin"
                            },
                            self.raw,
                        )
                        .on_click(cx.listener(|this, _, _w, cx| {
                            this.raw = !this.raw;
                            cx.notify();
                        }))
                    }))
                    .children((!document_mode).then(|| {
                        let snapshot = snapshot_for_filter.clone();
                        button(theme, "memory-kind", kind_label, self.kind_filter.is_some())
                            .on_click(cx.listener(move |this, _, _w, cx| {
                                if let Some(snapshot) = &snapshot {
                                    this.cycle_kind(snapshot, cx);
                                }
                            }))
                    }))
                    .children((!document_mode).then(|| {
                        button(
                            theme,
                            "memory-layer",
                            layer_label,
                            self.layer_filter.is_some(),
                        )
                        .on_click(cx.listener(|this, _, _w, cx| this.cycle_layer(cx)))
                    }))
                    .child(
                        button(theme, "memory-refresh", "Yenile", false)
                            .on_click(cx.listener(|_, _, _w, cx| cx.emit(MemoryEvent::Refresh))),
                    )
                    .child(
                        button(theme, "memory-close", "Kapat (Esc)", false)
                            .on_click(cx.listener(|_, _, _w, cx| cx.emit(MemoryEvent::Close))),
                    ),
            )
            .child(
                div()
                    .h(px(32.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(Input::new(&self.query)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(status),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(
                        div()
                            .id("memory-outline")
                            .w(px(300.0))
                            .flex_none()
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(gpui::rgb(theme.line))
                            .children(outline),
                    )
                    .child(
                        div()
                            .id("memory-detail")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(message.map(|m| {
                                div()
                                    .text_color(gpui::rgb(theme.accent_ink))
                                    .child(SharedString::from(m))
                            }))
                            .children(progress.map(|(done, all)| {
                                div().child(SharedString::from(format!(
                                    "{done}/{all} hedef tamamlandı · {} açık",
                                    all - done
                                )))
                            }))
                            .child(draw(theme, detail, family)),
                    ),
            )
    }
}
