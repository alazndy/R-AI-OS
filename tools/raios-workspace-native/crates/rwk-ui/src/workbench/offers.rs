//! Previous agent sessions: when a project opens, the recovery catalog is read and the agent
//! terminals the saved workspace remembers become offers in the dock. An offer is taken or
//! dismissed by the user; nothing here starts an agent on its own.
//!
//! Reading the catalog runs off the main thread and is generation-checked: a plan that arrives
//! after the project changed belongs to nothing. Taking an offer goes through the same launch
//! guard as a fresh agent — `raios resume <key>` starts only once the daemon has recorded the
//! launch in the project — and the offer is only retired when its pane exists, so a refusal
//! leaves it where it was.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gpui::{
    div, Context, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, Window,
};
use rwk_core::control::IdempotencyKey;
use rwk_core::guarded::Guarded;
use rwk_core::sessions::{ResumeKey, SessionLaunch};
use rwk_core::workbench::{
    parse_catalog, plan_offers, SavedTerminal, SessionCatalog, SessionOffer,
};

use super::Workbench;

/// How many catalog rows one project asks for (the CLI scans every project either way).
const CATALOG_TOP: &str = "30";

impl Workbench {
    /// Reads the recovery catalog of `root` and turns `agents` — the agent terminals of the
    /// state file that was just restored — into offers. Offers of a previous project are gone
    /// whichever way this ends.
    pub(super) fn load_offers(
        &mut self,
        agents: Vec<SavedTerminal>,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.offers.clear();
        self.resuming.clear();
        let generation = self.offers_generation;
        let raios = self.programs.raios.clone();
        // An offer nobody can see is an offer nobody takes; the saved dock state said it was
        // in use.
        self.dock_visible = true;
        cx.spawn_in(window, async move |this, cx| {
            let catalog = cx
                .background_executor()
                .spawn(async move { catalog_of(raios, &root) })
                .await;
            this.update_in(cx, |workbench, _window, cx| {
                workbench.apply_offers(generation, &agents, &catalog, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Publishes the offers of a plan that still belongs to the open project.
    fn apply_offers(
        &mut self,
        generation: u64,
        agents: &[SavedTerminal],
        catalog: &SessionCatalog,
        cx: &mut Context<Self>,
    ) {
        if generation != self.offers_generation {
            return;
        }
        let Some(project) = self.project.as_ref() else {
            return;
        };
        self.offers = plan_offers(agents, catalog, &project.info().root);
        cx.notify();
    }

    /// Takes an offer: the continuation goes through the launch guard, and only a validated
    /// launch starts its pane. An offer already on its way is not taken twice.
    pub fn resume_offer(
        &mut self,
        offer: &SessionOffer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(project), Some(matched)) = (self.project.clone(), offer.matched.as_ref()) else {
            return;
        };
        if !self.resuming.insert(matched.key.clone()) {
            return;
        }
        self.start_guarded(
            Guarded::ResumeAgent {
                root: project.info().root.clone(),
                key: matched.key.clone(),
                idempotency_key: IdempotencyKey::fresh(),
            },
            window,
            cx,
        );
        cx.notify();
    }

    /// Drops an offer: the session is not continued, and the next state file will not carry it.
    pub fn dismiss_offer(&mut self, offer: &SessionOffer, cx: &mut Context<Self>) {
        self.offers.retain(|kept| kept != offer);
        self.note_workspace_change(cx);
        cx.notify();
    }

    /// One guarded resume reached its end without starting a pane (denied, cancelled, failed);
    /// the offer may be taken again.
    pub(super) fn settle_resume(&mut self, op: &Guarded, cx: &mut Context<Self>) {
        if let Guarded::ResumeAgent { key, .. } = op {
            if self.resuming.remove(key) {
                cx.notify();
            }
        }
    }

    /// Starts the pane of a resume the guard allowed and retires the offer behind it. When the
    /// pane cannot start (the dock is full, the CLI is gone) the offer stays.
    pub(super) fn resume_terminal(
        &mut self,
        key: ResumeKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resuming.remove(&key);
        let kind = key.agent();
        if !self.spawn_terminal(kind, SessionLaunch::Resume(key.clone()), window, cx) {
            cx.notify();
            return;
        }
        self.offers
            .retain(|offer| offer.matched.as_ref().map(|matched| &matched.key) != Some(&key));
        self.after_started_terminal(window, cx);
    }

    /// The offers block of the dock: one row per previous session, above the terminals. A row
    /// whose catalog entry was found can be continued; any row can be dismissed.
    pub(super) fn offer_rows(&self, cx: &mut Context<Self>) -> Vec<impl gpui::IntoElement> {
        let theme = self.theme();
        self.offers
            .iter()
            .enumerate()
            .map(|(index, offer)| {
                let identity = offer.kind.identity().unwrap_or("ajan");
                let label = match &offer.matched {
                    Some(matched) => format!("Önceki oturum: {} ({identity})", matched.name),
                    None => format!("Önceki {identity} oturumu bulunamadı"),
                };
                let busy = offer
                    .matched
                    .as_ref()
                    .is_some_and(|matched| self.resuming.contains(&matched.key));
                let resumable = offer.matched.is_some() && !busy;
                let for_resume = offer.clone();
                let for_dismiss = offer.clone();
                let mut row = div()
                    .id(gpui::SharedString::from(format!("offer-{index}")))
                    .flex()
                    .items_center()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .bg(gpui::rgb(theme.panel))
                    .text_color(gpui::rgb(theme.text))
                    .border_b_1()
                    .border_color(gpui::rgb(theme.line))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(gpui::rgb(theme.muted))
                            .child(label),
                    );
                if resumable {
                    row = row.child(
                        div()
                            .id(gpui::SharedString::from(format!("offer-resume-{index}")))
                            .px_2()
                            .cursor_pointer()
                            .text_color(gpui::rgb(theme.accent_ink))
                            .child("Sürdür")
                            .on_click(cx.listener(move |workbench, _, window, cx| {
                                workbench.resume_offer(&for_resume, window, cx);
                            })),
                    );
                } else if busy {
                    row = row.child(
                        div()
                            .px_2()
                            .text_color(gpui::rgb(theme.muted))
                            .child("Başlatılıyor…"),
                    );
                }
                row.child(
                    div()
                        .id(gpui::SharedString::from(format!("offer-dismiss-{index}")))
                        .px_2()
                        .cursor_pointer()
                        .text_color(gpui::rgb(theme.muted))
                        .child("Vazgeç")
                        .on_click(cx.listener(move |workbench, _, _window, cx| {
                            workbench.dismiss_offer(&for_dismiss, cx);
                        })),
                )
            })
            .collect()
    }
}

/// Runs `raios resume --list --json --project <root>` and parses it. Every failure — no `raios`
/// on the machine, a CLI that errors, output that is not the catalog — is an empty catalog: the
/// offer rows still appear, without a session to continue.
fn catalog_of(raios: Option<PathBuf>, root: &Path) -> SessionCatalog {
    let executable = raios.unwrap_or_else(|| PathBuf::from("raios"));
    let output = Command::new(executable)
        .args(["resume", "--list", "--json", "--project"])
        .arg(root)
        .arg("--top")
        .arg(CATALOG_TOP)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => {
            parse_catalog(&String::from_utf8_lossy(&output.stdout)).unwrap_or_default()
        }
        _ => SessionCatalog::default(),
    }
}
