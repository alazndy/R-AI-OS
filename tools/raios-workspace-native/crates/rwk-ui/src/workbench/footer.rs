//! The footer: the toast on the left, clickable facts on the right (branch, problems, language
//! servers, terminals, save toggles, quota) and whatever the application added.

use gpui::{
    div, px, rgb, AppContext, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled,
};
use rwk_core::lsp::ServerStatus;
use rwk_core::workbench::{footer_items, upsert_footer_item, FooterInfo, FooterItem, FooterLevel};

use super::menus::Tip;
use super::Workbench;

fn level_color(level: FooterLevel, theme: rwk_core::workbench::Theme) -> u32 {
    match (level, theme.dark) {
        (FooterLevel::Normal, _) => theme.muted,
        (FooterLevel::Good, true) => 0x3fb950,
        (FooterLevel::Good, false) => 0x1a7f37,
        (FooterLevel::Warn, true) => 0xffa657,
        (FooterLevel::Warn, false) => 0xb35900,
        (FooterLevel::Bad, true) => 0xff7b72,
        (FooterLevel::Bad, false) => 0xc62828,
        (FooterLevel::Dim, _) => theme.line,
    }
}

impl Workbench {
    /// Adds a footer item of the application's own; an item with a known id replaces it.
    pub fn set_footer_item(&mut self, item: FooterItem, cx: &mut Context<Self>) {
        upsert_footer_item(&mut self.extra_footer, item);
        cx.notify();
    }

    /// Removes the application's footer item `id`.
    pub fn remove_footer_item(&mut self, id: &str, cx: &mut Context<Self>) {
        self.extra_footer.retain(|item| item.id != id);
        cx.notify();
    }

    /// Everything the footer shows, left to right.
    pub fn footer_items(&self, cx: &gpui::App) -> Vec<FooterItem> {
        let mut items = footer_items(&FooterInfo {
            project_open: self.project.is_some(),
            branch: self.branch.clone(),
            problems: self.problems.counts(),
            servers: self.server_status.clone(),
            dirty_files: self.editor.read(cx).dirty_paths().len(),
            terminals: self.terminals.len(),
            running: self.running_sessions(),
            auto_save: self.auto_save,
            format_on_save: self.format_on_save,
            quota: self.quota.clone(),
        });
        for item in &self.extra_footer {
            upsert_footer_item(&mut items, item.clone());
        }
        items
    }

    /// Reads the checked-out branch again.
    pub(super) fn refresh_branch(&mut self, cx: &mut Context<Self>) {
        let branch = self
            .project
            .as_ref()
            .and_then(|project| rwk_core::git::head_branch(&project.info().root));
        if branch != self.branch {
            self.branch = branch;
            cx.notify();
        }
    }

    /// Remembers a language server's state for the footer.
    pub(super) fn note_server_status(&mut self, server: &str, status: ServerStatus) {
        match self
            .server_status
            .iter_mut()
            .find(|(name, _)| name == server)
        {
            Some(known) => known.1 = status,
            None => self.server_status.push((server.to_owned(), status)),
        }
    }

    /// The footer bar element.
    pub(super) fn footer_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let toast = self.toast.clone().unwrap_or_default();
        let items = self.footer_items(cx).into_iter().map(|item| {
            let color = level_color(item.level, theme);
            let command = item.command;
            let tip = SharedString::from(item.tip.clone());
            let element = div()
                .id(SharedString::from(format!("footer-{}", item.id)))
                .flex_none()
                .px_1()
                .rounded_sm()
                .text_color(rgb(color))
                .tooltip(move |_window, cx| cx.new(|_| Tip(tip.clone())).into());
            match command {
                Some(command) => element
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.raised)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run(command, window, cx);
                    }))
                    .child(item.text)
                    .into_any_element(),
                None => element.child(item.text).into_any_element(),
            }
        });
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(div().flex_1().min_w_0().child(toast))
            .children(items)
            .h(px(18.0))
    }
}
