//! The inspector: tasks, running agents and pending approvals of the open project, read from the
//! daemon every few seconds. Read-only; approvals can be seen, not decided (spec section 8).
//!
//! The daemon is read on the background executor. Every result carries the generation of the
//! project it was requested for, so a slow answer for the previous project is dropped.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{div, Context, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window};
use gpui::{InteractiveElement, StatefulInteractiveElement};
use rwk_core::control::{ConnectionState, ControlPlane};
use rwk_core::inspector::{fetch, Banner, InspectorModel};

/// How often the daemon is read.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// What the inspector asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorEvent {
    /// The user asked to start the receiver of this handoff.
    StartReceiver(String),
}

impl gpui::EventEmitter<InspectorEvent> for InspectorView {}

/// One handoff line: approval id, text, and whether the receiver may be started.
pub type HandoffLine = (String, String, bool);

/// The inspector panel.
pub struct InspectorView {
    handoffs: Vec<HandoffLine>,
    plane: Option<Arc<dyn ControlPlane>>,
    project: Option<PathBuf>,
    /// Bumped on every project or plane change; answers of an older generation are dropped.
    generation: u64,
    model: InspectorModel,
    _poll: Option<Task<()>>,
}

impl InspectorView {
    /// An inspector with no project and no daemon connection.
    pub fn new() -> Self {
        Self {
            handoffs: Vec::new(),
            plane: None,
            project: None,
            generation: 0,
            model: InspectorModel::empty(&not_configured()),
            _poll: None,
        }
    }

    /// Sets (or removes) the daemon connection and restarts polling.
    pub fn set_control_plane(
        &mut self,
        plane: Option<Arc<dyn ControlPlane>>,
        cx: &mut Context<Self>,
    ) {
        self.plane = plane;
        self.restart(cx);
    }

    /// Shows the project at `root`, or nothing for `None`, and restarts polling.
    pub fn set_project(&mut self, root: Option<PathBuf>, cx: &mut Context<Self>) {
        self.project = root;
        self.restart(cx);
    }

    /// Replaces the handoff lines the workspace queued.
    pub fn set_handoffs(&mut self, lines: Vec<HandoffLine>, cx: &mut Context<Self>) {
        self.handoffs = lines;
        cx.notify();
    }

    /// What the panel currently shows.
    pub fn model(&self) -> &InspectorModel {
        &self.model
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self._poll = None;
        self.model = InspectorModel::empty(&if self.plane.is_some() {
            ConnectionState::Connected
        } else {
            not_configured()
        });
        let (Some(plane), Some(project)) = (self.plane.clone(), self.project.clone()) else {
            cx.notify();
            return;
        };
        let generation = self.generation;
        self._poll = Some(cx.spawn(async move |this, cx| loop {
            let plane = Arc::clone(&plane);
            let root = project.clone();
            let model = cx
                .background_executor()
                .spawn(async move { fetch(plane.as_ref(), &root) })
                .await;
            let alive = this.update(cx, |view, cx| {
                if view.generation == generation {
                    view.model = model;
                    cx.notify();
                }
            });
            if alive.is_err() {
                return;
            }
            cx.background_executor().timer(POLL_INTERVAL).await;
        }));
        cx.notify();
    }
}

impl Default for InspectorView {
    fn default() -> Self {
        Self::new()
    }
}

fn not_configured() -> ConnectionState {
    ConnectionState::Unreachable {
        reason: "daemon bağlantısı yapılandırılmadı".to_owned(),
    }
}

fn heading(text: &str, count: usize) -> impl IntoElement {
    div()
        .px_2()
        .pt_2()
        .text_xs()
        .text_color(gpui::rgb(0x9a9a9a))
        .child(SharedString::from(format!(
            "{} ({count})",
            text.to_uppercase()
        )))
}

fn row(primary: String, secondary: String) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .child(div().child(SharedString::from(primary)))
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(0x8a8a8a))
                .child(SharedString::from(secondary)),
        )
}

impl Render for InspectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let banner = self.model.banner.text().map(|text| {
            let color = match self.model.banner {
                Banner::Unreachable(_) => 0xf14c4c,
                _ => 0xcca700,
            };
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(gpui::rgb(color))
                .child(SharedString::from(text))
        });
        let tasks = self.model.tasks.iter().map(|task| {
            row(
                task.title.clone(),
                format!(
                    "{} · {}",
                    task.status,
                    task.assignee.as_deref().unwrap_or("atanmamış")
                ),
            )
        });
        let runs = self.model.runs.iter().map(|run| {
            row(
                run.agent.clone(),
                format!("çalışıyor · görev {}", run.task_id),
            )
        });
        let approvals = self.model.approvals.iter().map(|approval| {
            row(
                approval.title.clone(),
                format!(
                    "{} → {} · {}",
                    approval.from_agent, approval.to_agent, approval.reason
                ),
            )
        });
        let handoffs: Vec<_> = self
            .handoffs
            .iter()
            .map(|(id, text, can_start)| {
                let line = row(text.clone(), format!("onay {id}"));
                let start = can_start.then(|| {
                    let id = id.clone();
                    div()
                        .id(SharedString::from(format!("start-receiver-{id}")))
                        .mx_2()
                        .px_2()
                        .border_1()
                        .border_color(gpui::rgb(0x3794ff))
                        .text_xs()
                        .child("Alıcıyı başlat")
                        .on_click(cx.listener(move |_, _, _window, cx| {
                            cx.emit(InspectorEvent::StartReceiver(id.clone()));
                        }))
                });
                div().child(line).children(start)
            })
            .collect();
        let empty = (self.model.is_empty()
            && self.handoffs.is_empty()
            && matches!(self.model.banner, Banner::None))
        .then(|| {
            div()
                .px_2()
                .py_2()
                .text_xs()
                .child("Bu proje için kayıt yok.")
        });
        div()
            .id("inspector")
            .size_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(
                div()
                    .h(gpui::px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(gpui::rgb(0x333333))
                    .text_xs()
                    .text_color(gpui::rgb(0x9a9a9a))
                    .child("DENETÇİ"),
            )
            .children(banner)
            .child(heading("Onaylar", self.model.approvals.len()))
            .children(approvals)
            .child(heading("Çalışanlar", self.model.runs.len()))
            .children(runs)
            .child(heading("Görevler", self.model.tasks.len()))
            .children(tasks)
            .child(heading("Devirler", self.handoffs.len()))
            .children(handoffs)
            .children(empty)
            .children((self.model.unscoped > 0).then(|| {
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(gpui::rgb(0x8a8a8a))
                    .child(SharedString::from(format!(
                        "{} kayıt bir projeye bağlı olmadığı için gösterilmiyor.",
                        self.model.unscoped
                    )))
            }))
    }
}
