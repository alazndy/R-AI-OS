//! The inspector: tasks, running agents and pending approvals of the open project, read from the
//! daemon every few seconds. Read-only; approvals can be seen, not decided (spec section 8).
//!
//! The daemon is read on the background executor. Every result carries the generation of the
//! project it was requested for, so a slow answer for the previous project is dropped.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gpui::{div, Context, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window};
use gpui::{InteractiveElement, StatefulInteractiveElement};
use rwk_core::control::{ConnectionState, ControlEvent, ControlPlane, EventSource};
use rwk_core::inspector::{fetch, Banner, InspectorModel};
use rwk_core::workbench::{Appearance, Theme};

/// How often the daemon is read.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// How long one monitor call may block before the thread checks whether it should stop.
const EVENT_SLICE: Duration = Duration::from_millis(250);
/// How long the monitor waits after a failing token read before it tries again.
const TOKEN_RETRY: Duration = Duration::from_secs(2);

/// What the inspector asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectorEvent {
    /// The user asked to start the receiver of this handoff.
    StartReceiver(String),
    /// The user approved a pending handoff approval, echoing the revision that was shown.
    ApproveHandoff {
        /// Target approval identifier.
        approval_id: String,
        /// Revision the row was displayed at.
        revision: i64,
    },
    /// The user wants to reject a pending handoff approval; a reason is asked for before sending.
    RejectHandoff {
        /// Target approval identifier.
        approval_id: String,
        /// Revision the row was displayed at.
        revision: i64,
    },
}

impl gpui::EventEmitter<InspectorEvent> for InspectorView {}

/// One handoff line: approval id, text, and whether the receiver may be started.
pub type HandoffLine = (String, String, bool);

/// The inspector panel.
pub struct InspectorView {
    handoffs: Vec<HandoffLine>,
    theme: Theme,
    plane: Option<Arc<dyn ControlPlane>>,
    project: Option<PathBuf>,
    /// Bumped on every project or plane change; answers of an older generation are dropped.
    generation: u64,
    model: InspectorModel,
    /// Why the live event stream is down, `None` while it works or none is configured.
    stream_down: Option<String>,
    /// The running monitor of the configured event stream.
    stream_watch: Option<StreamWatch>,
    /// The task that applies stream signals to this view.
    _stream_notifications: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

impl InspectorView {
    /// An inspector with no project and no daemon connection.
    pub fn new() -> Self {
        Self {
            handoffs: Vec::new(),
            theme: Theme::of(Appearance::default()),
            plane: None,
            project: None,
            generation: 0,
            model: InspectorModel::empty(&not_configured()),
            stream_down: None,
            stream_watch: None,
            _stream_notifications: None,
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

    /// Repaints the panel with another skin or tone.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
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

    /// Connects (or removes) the daemon's live event stream. The stream only accelerates the
    /// panel: polling stays on regardless, so a missing or broken stream never hides anything.
    pub fn set_event_source(
        &mut self,
        source: Option<Box<dyn EventSource>>,
        cx: &mut Context<Self>,
    ) {
        self.stream_watch = None;
        self._stream_notifications = None;
        self.stream_down = None;
        let Some(source) = source else {
            cx.notify();
            return;
        };
        let (sender, receiver) = async_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        self.stream_watch = Some(StreamWatch::start(source, sender, Arc::clone(&stop)));
        self._stream_notifications = Some(cx.spawn(async move |this, cx| {
            while let Ok(signal) = receiver.recv().await {
                // A burst of events (a handoff publishes several) is one refresh.
                let mut signals = vec![signal];
                while let Ok(next) = receiver.try_recv() {
                    signals.push(next);
                }
                let alive = this.update(cx, |view, cx| view.apply_stream_signals(signals, cx));
                if alive.is_err() {
                    return;
                }
            }
        }));
        cx.notify();
    }

    /// The line above the lists: the query state wins over the stream state, because a stale
    /// list matters more than a missing accelerator. When only the stream is down, the note
    /// says the poll still runs. The flag marks an alarm (red) rather than a note.
    fn visible_banner(&self) -> Option<(String, bool)> {
        let banner = &self.model.banner;
        match banner {
            Banner::Unreachable(_) => banner.text().map(|text| (text, true)),
            Banner::Degraded(_) => banner.text().map(|text| (text, false)),
            Banner::None => self
                .stream_down
                .as_ref()
                .map(|reason| (format!("Canlı akış yok (yoklama sürüyor): {reason}"), false)),
        }
    }

    /// Applies the signals one monitor batch carried: the note changes, a refresh is asked for.
    fn apply_stream_signals(&mut self, signals: Vec<StreamSignal>, cx: &mut Context<Self>) {
        let previous = self.stream_down.clone();
        let mut refresh = false;
        for signal in signals {
            match signal {
                StreamSignal::Connected => self.stream_down = None,
                StreamSignal::Down(reason) => self.stream_down = Some(reason),
                StreamSignal::Event => refresh = true,
            }
        }
        if refresh {
            self.refresh_now(cx);
        }
        if refresh || self.stream_down != previous {
            cx.notify();
        }
    }

    /// One extra read of the daemon right after a typed event, instead of waiting for the poll.
    pub(crate) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        let (Some(plane), Some(project)) = (self.plane.clone(), self.project.clone()) else {
            return;
        };
        let generation = self.generation;
        cx.spawn(async move |this, cx| {
            let model = cx
                .background_executor()
                .spawn(async move { fetch(plane.as_ref(), &project) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.generation == generation {
                    view.model = model;
                    cx.notify();
                }
            });
        })
        .detach();
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

/// What the monitor thread sends to the view.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamSignal {
    /// The stream connected (or reconnected): the note goes away.
    Connected,
    /// The stream is down; the reason is shown while the poll keeps the panel fresh.
    Down(String),
    /// A typed daemon event arrived; refresh now instead of at the next poll.
    Event,
}

/// One monitor thread around one event source.
struct StreamWatch {
    stop: Arc<AtomicBool>,
    _thread: std::thread::JoinHandle<()>,
}

impl StreamWatch {
    /// Runs `source` on its own thread, forwarding each event as a [`StreamSignal`]. The thread
    /// blocks at most [`EVENT_SLICE`] per call and stops within one slice of `stop` being set.
    fn start(
        mut source: Box<dyn EventSource>,
        sender: async_channel::Sender<StreamSignal>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match source.next_event(EVENT_SLICE) {
                    Ok(Some(event)) => {
                        let Some(signal) = signal_for(&event) else {
                            continue;
                        };
                        if sender.send_blocking(signal).is_err() {
                            return; // the view is gone
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        // The event source cannot be retried locally (a missing token file,
                        // above all): report it and try again slowly, the poll carries on.
                        if sender
                            .send_blocking(StreamSignal::Down(error.message))
                            .is_err()
                        {
                            return;
                        }
                        std::thread::sleep(TOKEN_RETRY);
                    }
                }
            }
        });
        Self {
            stop,
            _thread: thread,
        }
    }
}

impl Drop for StreamWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The signal for one control event; `None` when the event needs no reaction.
fn signal_for(event: &ControlEvent) -> Option<StreamSignal> {
    match event {
        ControlEvent::ConnectionChanged(ConnectionState::Connected) => {
            Some(StreamSignal::Connected)
        }
        ControlEvent::ConnectionChanged(ConnectionState::Degraded { reason })
        | ControlEvent::ConnectionChanged(ConnectionState::Unreachable { reason }) => {
            Some(StreamSignal::Down(reason.clone()))
        }
        ControlEvent::SnapshotChanged { .. }
        | ControlEvent::RunChanged { .. }
        | ControlEvent::ApprovalChanged { .. } => Some(StreamSignal::Event),
    }
}

fn heading(theme: Theme, text: &str, count: usize) -> impl IntoElement {
    div()
        .px_2()
        .pt_2()
        .text_xs()
        .text_color(gpui::rgb(if theme.lcars {
            theme.accent_ink
        } else {
            theme.muted
        }))
        .child(SharedString::from(format!(
            "{} ({count})",
            text.to_uppercase()
        )))
}

fn row(theme: Theme, primary: String, secondary: String) -> impl IntoElement {
    div()
        .px_2()
        .py_0p5()
        .child(div().child(SharedString::from(primary)))
        .child(
            div()
                .text_xs()
                .text_color(gpui::rgb(theme.muted))
                .child(SharedString::from(secondary)),
        )
}

impl Render for InspectorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let banner = self.visible_banner().map(|(text, alarm)| {
            let color = if alarm { theme.bad } else { theme.accent_ink };
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(gpui::rgb(color))
                .child(SharedString::from(text))
        });
        let tasks = self.model.tasks.iter().map(|task| {
            row(
                theme,
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
                theme,
                run.agent.clone(),
                format!("çalışıyor · görev {}", run.task_id),
            )
        });
        let approvals = self.model.approvals.iter().map(|approval| {
            let line = row(
                theme,
                approval.title.clone(),
                format!(
                    "{} → {} · {}",
                    approval.from_agent, approval.to_agent, approval.reason
                ),
            );
            // Only handoff approvals can be decided here; other kinds stay read-only.
            let decisions = (approval.kind == "handover").then(|| {
                let approve_id = approval.id.clone();
                let reject_id = approval.id.clone();
                let revision = approval.revision;
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .px_2()
                    .pb_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("approve-handoff-{approve_id}")))
                            .px_2()
                            .border_1()
                            .border_color(gpui::rgb(theme.accent_ink))
                            .text_xs()
                            .child("Onayla")
                            .on_click(cx.listener(move |_, _, _window, cx| {
                                cx.emit(InspectorEvent::ApproveHandoff {
                                    approval_id: approve_id.clone(),
                                    revision,
                                });
                            })),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("reject-handoff-{reject_id}")))
                            .px_2()
                            .border_1()
                            .border_color(gpui::rgb(theme.bad))
                            .text_xs()
                            .child("Reddet")
                            .on_click(cx.listener(move |_, _, _window, cx| {
                                cx.emit(InspectorEvent::RejectHandoff {
                                    approval_id: reject_id.clone(),
                                    revision,
                                });
                            })),
                    )
            });
            div().child(line).children(decisions)
        });
        let handoffs: Vec<_> = self
            .handoffs
            .iter()
            .map(|(id, text, can_start)| {
                let line = row(theme, text.clone(), format!("onay {id}"));
                let start = can_start.then(|| {
                    let id = id.clone();
                    div()
                        .id(SharedString::from(format!("start-receiver-{id}")))
                        .mx_2()
                        .px_2()
                        .border_1()
                        .border_color(gpui::rgb(theme.accent_ink))
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
                    .border_color(gpui::rgb(theme.line))
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child("DENETÇİ"),
            )
            .children(banner)
            .child(heading(theme, "Onaylar", self.model.approvals.len()))
            .children(approvals)
            .child(heading(theme, "Çalışanlar", self.model.runs.len()))
            .children(runs)
            .child(heading(theme, "Görevler", self.model.tasks.len()))
            .children(tasks)
            .child(heading(theme, "Devirler", self.handoffs.len()))
            .children(handoffs)
            .children(empty)
            .children((self.model.unscoped > 0).then(|| {
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(gpui::rgb(theme.muted))
                    .child(SharedString::from(format!(
                        "{} kayıt bir projeye bağlı olmadığı için gösterilmiyor.",
                        self.model.unscoped
                    )))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use rwk_core::error::{AppError, ErrorCode};
    use rwk_core::Result;

    fn view_with(banner: Banner, stream: Option<&str>) -> InspectorView {
        let mut view = InspectorView::new();
        view.model.banner = banner;
        view.stream_down = stream.map(str::to_owned);
        view
    }

    #[test]
    fn the_query_state_wins_the_banner_over_the_stream() {
        let view = view_with(
            Banner::Unreachable("bağlantı reddedildi".into()),
            Some("koptu"),
        );
        let (text, alarm) = view.visible_banner().expect("banner");
        assert!(text.contains("ulaşılamıyor") && text.contains("bağlantı reddedildi"));
        assert!(alarm);

        let view = view_with(Banner::Degraded("yavaş".into()), Some("koptu"));
        let (text, alarm) = view.visible_banner().expect("banner");
        assert!(text.contains("yavaş") && !text.contains("koptu"));
        assert!(!alarm);
    }

    #[test]
    fn a_stream_only_failure_is_a_note_that_the_poll_runs() {
        let view = view_with(Banner::None, Some("bağlantı reddedildi"));
        let (text, alarm) = view.visible_banner().expect("note");
        assert!(text.contains("Canlı akış yok") && text.contains("yoklama sürüyor"));
        assert!(text.contains("bağlantı reddedildi"));
        assert!(!alarm);

        assert_eq!(view_with(Banner::None, None).visible_banner(), None);
    }

    #[test]
    fn control_events_map_to_stream_signals() {
        assert_eq!(
            signal_for(&ControlEvent::ConnectionChanged(ConnectionState::Connected)),
            Some(StreamSignal::Connected)
        );
        assert_eq!(
            signal_for(&ControlEvent::ConnectionChanged(
                ConnectionState::Unreachable {
                    reason: "koptu".into(),
                }
            )),
            Some(StreamSignal::Down("koptu".into()))
        );
        assert_eq!(
            signal_for(&ControlEvent::ConnectionChanged(
                ConnectionState::Degraded {
                    reason: "yavaş".into(),
                }
            )),
            Some(StreamSignal::Down("yavaş".into()))
        );
        assert_eq!(
            signal_for(&ControlEvent::SnapshotChanged { sequence: 3 }),
            Some(StreamSignal::Event)
        );
        assert_eq!(
            signal_for(&ControlEvent::RunChanged {
                run_id: "r1".into(),
                agent_name: "codex_kaira".into(),
                status: "running".into(),
            }),
            Some(StreamSignal::Event)
        );
        assert_eq!(
            signal_for(&ControlEvent::ApprovalChanged {
                approval_id: "a1".into(),
            }),
            Some(StreamSignal::Event)
        );
    }

    #[test]
    fn the_monitor_forwards_events_and_reports_a_failing_token() {
        struct ScriptedEvents {
            queue: Mutex<VecDeque<Result<Option<ControlEvent>>>>,
        }

        impl EventSource for ScriptedEvents {
            fn next_event(&mut self, _timeout: Duration) -> Result<Option<ControlEvent>> {
                self.queue
                    .lock()
                    .expect("lock")
                    .pop_front()
                    .unwrap_or(Ok(None))
            }
        }

        let mut queue = VecDeque::new();
        queue.push_back(Ok(Some(ControlEvent::RunChanged {
            run_id: "r1".into(),
            agent_name: "codex_kaira".into(),
            status: "running".into(),
        })));
        queue.push_back(Ok(Some(ControlEvent::ConnectionChanged(
            ConnectionState::Unreachable {
                reason: "koptu".into(),
            },
        ))));
        queue.push_back(Err(AppError::new(ErrorCode::Unauthorized, "jeton yok")));

        let (sender, receiver) = async_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let source: Box<dyn EventSource> = Box::new(ScriptedEvents {
            queue: Mutex::new(queue),
        });
        let watch = StreamWatch::start(source, sender, Arc::clone(&stop));

        assert_eq!(
            receiver.recv_blocking().expect("signal"),
            StreamSignal::Event
        );
        assert_eq!(
            receiver.recv_blocking().expect("signal"),
            StreamSignal::Down("koptu".into())
        );
        assert_eq!(
            receiver.recv_blocking().expect("signal"),
            StreamSignal::Down("jeton yok".into())
        );
        drop(watch);
        assert!(stop.load(Ordering::Relaxed));
    }
}
