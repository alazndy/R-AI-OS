//! The SSH picker: a query box over the hosts the user already has (`~/.ssh/config`,
//! `known_hosts`). Whatever is typed that is a valid target can be connected to as well, so
//! `user@10.0.0.7` works without being in any file. The picker only validates and reports; the
//! workbench starts the terminal.

use gpui::{
    div, App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window,
};
use gpui_base::input::{InputEvent, InputState};
use gpui_base::Input;
use rwk_core::ssh::{filter_hosts, SshHost, SshTarget};
use rwk_core::workbench::{Appearance, Theme};

/// What the picker asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshPickerEvent {
    /// Connect to this target.
    Connect(SshTarget),
    /// Close without connecting.
    Close,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshRow {
    /// The text on the row.
    pub label: String,
    /// What the row connects to.
    pub target: SshTarget,
    /// The row is the typed text rather than a known host.
    pub typed: bool,
}

impl EventEmitter<SshPickerEvent> for SshPicker {}

/// The picker view.
pub struct SshPicker {
    input: Entity<InputState>,
    hosts: Vec<SshHost>,
    rows: Vec<SshRow>,
    selected: usize,
    scroll: ScrollHandle,
    theme: Theme,
    _subscription: Subscription,
}

/// Rows for `query` over `hosts`: the known hosts that match, then the typed text as a row of
/// its own when it is a valid target that no listed host already spells exactly. When nothing
/// matches, the typed row is the only one.
pub fn rows_for(hosts: &[SshHost], query: &str) -> Vec<SshRow> {
    let mut rows: Vec<SshRow> = filter_hosts(hosts, query)
        .into_iter()
        .filter_map(|host| {
            let target = host.target()?;
            let label = match &host.detail {
                Some(detail) => format!("{}   {detail}", host.alias),
                None => host.alias.clone(),
            };
            Some(SshRow {
                label,
                target,
                typed: false,
            })
        })
        .collect();
    let typed = query.trim();
    if !typed.is_empty() {
        if let Ok(target) = SshTarget::parse(typed) {
            if !hosts.iter().any(|host| host.alias == typed) {
                rows.push(SshRow {
                    label: format!("Bağlan: {}", target.display()),
                    target,
                    typed: true,
                });
            }
        }
    }
    rows
}

impl SshPicker {
    /// A picker over `hosts`; the query box takes focus.
    pub fn new(hosts: Vec<SshHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).submit_on_enter(true));
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscription =
            cx.subscribe(&input, |this, _input, event: &InputEvent, cx| match event {
                InputEvent::Change => this.refilter(cx),
                InputEvent::PressEnter { .. } => this.confirm(cx),
                InputEvent::Focus | InputEvent::Blur => {}
            });
        let rows = rows_for(&hosts, "");
        Self {
            input,
            hosts,
            rows,
            selected: 0,
            scroll: ScrollHandle::new(),
            theme: Theme::of(Appearance::default()),
            _subscription: subscription,
        }
    }

    /// Repaints the picker with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
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
    pub fn rows(&self) -> &[SshRow] {
        &self.rows
    }

    /// Index of the highlighted row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.rows = rows_for(&self.hosts, &query);
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    /// Moves the highlight, wrapping at both ends.
    pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.rows.len() as isize;
        if count == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(count) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    /// Connects to the highlighted row.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(self.selected) {
            cx.emit(SshPickerEvent::Connect(row.target.clone()));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SshPickerEvent::Close),
            "down" => self.move_selection(1, cx),
            "up" => self.move_selection(-1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// Height of the query box.
const INPUT_HEIGHT: f32 = 32.0;

impl Render for SshPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let chosen = row.target.clone();
                let marked = index == self.selected;
                div()
                    .id(SharedString::from(format!("ssh-row-{index}")))
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(gpui::rgb(if row.typed {
                        theme.accent_ink
                    } else {
                        theme.text
                    }))
                    .when_marked(marked, theme)
                    .hover(move |style| style.bg(gpui::rgb(theme.raised)))
                    .child(SharedString::from(format!(
                        "{}{}",
                        if marked { "› " } else { "  " },
                        row.label
                    )))
                    .on_click(cx.listener(move |_, _, _window, cx| {
                        cx.emit(SshPickerEvent::Connect(chosen.clone()));
                    }))
            })
            .collect();
        let hint = if self.hosts.is_empty() && self.rows.is_empty() {
            "~/.ssh içinde sunucu bulunamadı; kullanıcı@sunucu[:port] yazıp Enter'a basın"
        } else if self.rows.is_empty() {
            "Eşleşen sunucu yok; kullanıcı@sunucu[:port] yazıp Enter'a basın"
        } else {
            "Enter bağlanır, ↑↓ gezer, Esc kapatır"
        };
        div()
            .absolute()
            .occlude()
            .child(
                crate::overlay::close_button(self.theme, "ssh-close")
                    .on_click(cx.listener(|_, _, _window, cx| cx.emit(SshPickerEvent::Close))),
            )
            .top_8()
            .left_0()
            .right_0()
            .mx_auto()
            .w(gpui::px(560.0))
            .p_3()
            .rounded_md()
            .shadow_lg()
            .bg(gpui::rgb(theme.panel))
            .border_1()
            .border_color(gpui::rgb(theme.line))
            .text_color(gpui::rgb(theme.text))
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(SharedString::from("SSH ile bağlan"))
            .child(
                div()
                    .h(gpui::px(INPUT_HEIGHT))
                    .overflow_hidden()
                    .child(Input::new(&self.input)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(hint),
            )
            .child(
                div()
                    .id("ssh-results")
                    .max_h(gpui::px(320.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(rows),
            )
    }
}

/// Styling of the highlighted row.
trait WhenMarked: Styled + Sized {
    fn when_marked(self, marked: bool, theme: Theme) -> Self {
        if marked {
            self.bg(gpui::rgb(if theme.lcars {
                theme.accent
            } else {
                theme.selected
            }))
        } else {
            self
        }
    }
}

impl<T: Styled + Sized> WhenMarked for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(alias: &str, detail: Option<&str>) -> SshHost {
        SshHost {
            alias: alias.into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn typed_targets_are_offered_unless_a_known_host_already_spells_them() {
        let hosts = vec![host("prod", Some("deploy@10.0.0.1")), host("lab", None)];
        let labels = |query: &str| -> Vec<(String, bool)> {
            rows_for(&hosts, query)
                .into_iter()
                .map(|row| (row.label, row.typed))
                .collect()
        };

        // Empty query: every known host, no typed row.
        assert_eq!(labels("").len(), 2);
        // An exact alias is the known host, not a second "Bağlan" row.
        assert_eq!(
            labels("prod"),
            [("prod   deploy@10.0.0.1".to_owned(), false)]
        );
        // A new target is offered after the matches (here there are none).
        assert_eq!(
            labels("me@10.9.9.9:2200"),
            [("Bağlan: me@10.9.9.9:2200".to_owned(), true)]
        );
        // Text that is not a target offers nothing.
        assert!(labels("-oProxyCommand=x").is_empty());
        assert!(labels("two words").is_empty());
    }
}
