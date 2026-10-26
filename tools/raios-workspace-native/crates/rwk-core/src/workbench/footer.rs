//! The footer: small facts about the workspace, each optionally a button. The list is computed
//! from plain state, so what shows and what a click does is testable without a window, and the
//! application can add items of its own next to the built-in ones.

use super::commands::CommandId;
use crate::lsp::{Counts, ServerStatus};

/// How an item is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterLevel {
    /// Ordinary information.
    Normal,
    /// Something is fine.
    Good,
    /// Needs a look.
    Warn,
    /// Something is wrong.
    Bad,
    /// Switched off or inactive.
    Dim,
}

/// One footer item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterItem {
    /// Stable identifier; adding an item with the id of an existing one replaces it.
    pub id: String,
    /// What is shown.
    pub text: String,
    /// The tooltip.
    pub tip: String,
    /// What a click runs, if anything.
    pub command: Option<CommandId>,
    /// Its colour.
    pub level: FooterLevel,
}

impl FooterItem {
    /// An item that only shows `text`.
    pub fn info(id: &str, text: impl Into<String>, tip: impl Into<String>) -> Self {
        Self {
            id: id.to_owned(),
            text: text.into(),
            tip: tip.into(),
            command: None,
            level: FooterLevel::Normal,
        }
    }

    /// Makes a click run `command`.
    pub fn on_click(mut self, command: CommandId) -> Self {
        self.command = Some(command);
        self
    }

    /// Sets the colour.
    pub fn level(mut self, level: FooterLevel) -> Self {
        self.level = level;
        self
    }
}

/// Everything the built-in footer items are made from.
#[derive(Debug, Clone, Default)]
pub struct FooterInfo {
    /// A project is open.
    pub project_open: bool,
    /// The checked-out git branch.
    pub branch: Option<String>,
    /// Totals of the language servers' problems.
    pub problems: Counts,
    /// The state of each language server that was started (catalog name, state).
    pub servers: Vec<(String, ServerStatus)>,
    /// Files with unsaved edits.
    pub dirty_files: usize,
    /// Terminal panes, and how many of their processes still run.
    pub terminals: usize,
    /// See `terminals`.
    pub running: usize,
    /// Save on focus loss is on.
    pub auto_save: bool,
    /// Saving formats first.
    pub format_on_save: bool,
    /// The quota reading, when there is one.
    pub quota: Option<String>,
}

/// The built-in items, left to right.
pub fn footer_items(info: &FooterInfo) -> Vec<FooterItem> {
    let mut items = Vec::new();
    if let Some(branch) = &info.branch {
        items.push(
            FooterItem::info(
                "branch",
                format!("⎇ {branch}"),
                "Git dalı; değişiklikleri göster",
            )
            .on_click(CommandId::SourceControl),
        );
    }
    if info.project_open {
        let Counts { errors, warnings } = info.problems;
        let (text, level) = match (errors, warnings) {
            (0, 0) => ("✓ sorun yok".to_owned(), FooterLevel::Good),
            (e, 0) => (format!("✖ {e}"), FooterLevel::Bad),
            (0, w) => (format!("▲ {w}"), FooterLevel::Warn),
            (e, w) => (format!("✖ {e}  ▲ {w}"), FooterLevel::Bad),
        };
        items.push(
            FooterItem::info("problems", text, "Dil sunucusunun bulduğu sorunlar")
                .on_click(CommandId::Problems)
                .level(level),
        );
    }
    for (name, status) in &info.servers {
        let (text, tip, level) = match status {
            ServerStatus::Starting => (
                format!("{name} …"),
                "Dil sunucusu başlıyor".to_owned(),
                FooterLevel::Dim,
            ),
            ServerStatus::Ready => (
                format!("{name} ✓"),
                "Dil sunucusu hazır".to_owned(),
                FooterLevel::Good,
            ),
            ServerStatus::Unavailable(_) => {
                let program = crate::lsp::spec_named(name).map_or("Sunucu", |spec| spec.program);
                (
                    format!("{name} yok"),
                    format!(
                        "{program} PATH'te bulunamadı; kurulunca bu dilde tamamlama ve sorunlar çalışır; tıklayınca yeniden dener"
                    ),
                    FooterLevel::Warn,
                )
            }
            ServerStatus::Crashed => (
                format!("{name} çöktü"),
                "Dil sunucusu çöktü ve yeniden başlatılamadı; tıklayınca yeniden dener".to_owned(),
                FooterLevel::Bad,
            ),
        };
        let mut item = FooterItem::info(&format!("lsp-{name}"), text, tip).level(level);
        if matches!(status, ServerStatus::Unavailable(_) | ServerStatus::Crashed) {
            item = item.on_click(CommandId::RestartServers);
        }
        items.push(item);
    }
    if info.dirty_files > 0 {
        items.push(
            FooterItem::info(
                "dirty",
                format!("● {} kaydedilmedi", info.dirty_files),
                "Kaydedilmemiş dosyalar; açık olanı kaydet",
            )
            .on_click(CommandId::Save)
            .level(FooterLevel::Warn),
        );
    }
    if info.project_open {
        let text = if info.running == info.terminals {
            format!("⌨ {}", info.terminals)
        } else {
            format!("⌨ {}/{}", info.running, info.terminals)
        };
        items.push(
            FooterItem::info(
                "terminals",
                text,
                "Terminaller (çalışan/toplam); dock'u göster",
            )
            .on_click(CommandId::ToggleTerminal),
        );
        for (id, on, label, command) in [
            (
                "auto-save",
                info.auto_save,
                "Oto-kayıt",
                CommandId::ToggleAutoSave,
            ),
            (
                "format-on-save",
                info.format_on_save,
                "Biçim",
                CommandId::ToggleFormatOnSave,
            ),
        ] {
            items.push(
                FooterItem::info(
                    id,
                    format!("{label} {}", if on { "açık" } else { "kapalı" }),
                    "Aç/kapat",
                )
                .on_click(command)
                .level(if on {
                    FooterLevel::Good
                } else {
                    FooterLevel::Dim
                }),
            );
        }
    }
    if let Some(quota) = &info.quota {
        items.push(FooterItem::info("quota", quota.clone(), "Kota"));
    }
    items
}

/// Adds `item`, replacing the one with the same id.
pub fn upsert(items: &mut Vec<FooterItem>, item: FooterItem) {
    match items.iter_mut().find(|known| known.id == item.id) {
        Some(known) => *known = item,
        None => items.push(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(items: &[FooterItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn nothing_but_the_quota_shows_without_a_project() {
        let items = footer_items(&FooterInfo {
            quota: Some("%40".into()),
            ..FooterInfo::default()
        });
        assert_eq!(ids(&items), ["quota"]);
    }

    #[test]
    fn the_items_follow_the_state_and_click_through_to_commands() {
        let items = footer_items(&FooterInfo {
            project_open: true,
            branch: Some("main".into()),
            problems: Counts {
                errors: 2,
                warnings: 1,
            },
            servers: vec![
                (
                    "rust".into(),
                    ServerStatus::Unavailable("not installed".into()),
                ),
                ("python".into(), ServerStatus::Ready),
            ],
            dirty_files: 3,
            terminals: 4,
            running: 2,
            auto_save: true,
            format_on_save: false,
            quota: None,
        });
        assert_eq!(
            ids(&items),
            [
                "branch",
                "problems",
                "lsp-rust",
                "lsp-python",
                "dirty",
                "terminals",
                "auto-save",
                "format-on-save"
            ]
        );
        let by = |id: &str| items.iter().find(|item| item.id == id).unwrap();
        assert_eq!(by("branch").command, Some(CommandId::SourceControl));
        assert_eq!(by("problems").text, "✖ 2  ▲ 1");
        assert_eq!(by("problems").level, FooterLevel::Bad);
        assert_eq!(by("lsp-rust").level, FooterLevel::Warn);
        assert!(
            by("lsp-rust").tip.contains("rust-analyzer"),
            "{}",
            by("lsp-rust").tip
        );
        assert_eq!(by("lsp-rust").command, Some(CommandId::RestartServers));
        assert_eq!(by("dirty").text, "● 3 kaydedilmedi");
        assert_eq!(by("terminals").text, "⌨ 2/4");
        assert_eq!(by("auto-save").level, FooterLevel::Good);
        assert_eq!(by("format-on-save").level, FooterLevel::Dim);
        assert_eq!(
            by("format-on-save").command,
            Some(CommandId::ToggleFormatOnSave)
        );
    }

    #[test]
    fn an_item_with_a_known_id_replaces_it() {
        let mut items = vec![FooterItem::info("a", "1", "")];
        upsert(&mut items, FooterItem::info("b", "2", ""));
        upsert(&mut items, FooterItem::info("a", "3", ""));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "3");
    }
}
