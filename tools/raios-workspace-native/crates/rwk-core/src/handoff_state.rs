//! Pure model of one handoff's lifecycle as the inspector shows it.
//!
//! Queueing a handoff, starting the receiver and the wrapper consuming the handoff are three
//! separate pieces of evidence. This module replaces workspace 0.1's hand-written flags with
//! one function, [`next`], whose behavior is pinned by the transition table below.
//!
//! # Transition table
//!
//! A view is `(receipt, receiver, receipt_is_fresh)`. `receipt_is_fresh` means the most recent
//! receipt read happened after the last receiver exit or failed launch.
//!
//! | # | Observation | Applies when | Result |
//! |---|---|---|---|
//! | 1 | `QueueRequested` | receipt is `NotQueued` | receipt `Queuing` |
//! | 2 | `QueueSucceeded{id}` | receipt is `Queuing` | receipt `Pending{id}`, fresh |
//! | 3 | `QueueFailed` | receipt is `Queuing` | receipt `NotQueued` |
//! | 4 | `LaunchRequested` | [`can_start_receiver`] | receiver `Launching`, not fresh |
//! | 5 | `ReceiverRunning` | receiver is `Launching` | receiver `Running` |
//! | 6 | `LaunchFailed` | receiver is `Launching` | receiver `Exited`, not fresh |
//! | 7 | `ReceiverExited` | receiver is `Launching` or `Running` | receiver `Exited`, not fresh |
//! | 8 | `Receipt(o)` | receipt has an id and is not terminal | see below |
//!
//! Receipt observations (row 8): `Pending` becomes `Pending` (fresh); `DeliveredByTarget`
//! becomes `Delivered`; `ResolvedElsewhere`, `Expired` and `Rejected` become their states;
//! `NotFound` becomes `Missing` (not fresh); `ReadFailed` becomes `Unreadable` (not fresh).
//! `Missing` and `Unreadable` are recoverable by a later `Pending`. `Delivered`,
//! `ResolvedElsewhere`, `Expired` and `Rejected` are **terminal for that approval id**: no later
//! observation, including a missing or stale read, changes them. Observations that do not apply
//! leave the view unchanged.
//!
//! Only delivery by the addressed agent's own wrapper is `Delivered`. An approval resolved by
//! any other principal is `ResolvedElsewhere` and blocks a restart without claiming delivery.

/// What the daemon says about the handoff's receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptState {
    /// Nothing has been queued in this view.
    NotQueued,
    /// The queue command was sent and no receipt id exists yet.
    Queuing,
    /// Queued and still waiting for the receiver.
    Pending {
        /// Approval identifier of the queued handoff.
        approval_id: String,
    },
    /// Terminal: the addressed agent's wrapper consumed the handoff.
    Delivered {
        /// Approval identifier of the handoff.
        approval_id: String,
    },
    /// Terminal: resolved by another principal; not target delivery.
    ResolvedElsewhere {
        /// Approval identifier of the handoff.
        approval_id: String,
    },
    /// Terminal: superseded or expired.
    Expired {
        /// Approval identifier of the handoff.
        approval_id: String,
    },
    /// Terminal: rejected.
    Rejected {
        /// Approval identifier of the handoff.
        approval_id: String,
    },
    /// The receipt was not found for this project; recoverable.
    Missing {
        /// Approval identifier that was looked up.
        approval_id: String,
    },
    /// The receipt could not be read; recoverable.
    Unreadable {
        /// Approval identifier that was looked up.
        approval_id: String,
        /// Short reason for the banner.
        reason: String,
    },
}

impl ReceiptState {
    /// True for states no later observation may change.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ReceiptState::Delivered { .. }
                | ReceiptState::ResolvedElsewhere { .. }
                | ReceiptState::Expired { .. }
                | ReceiptState::Rejected { .. }
        )
    }
}

/// Liveness of the receiver process this app started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverState {
    /// No receiver has been started for this handoff.
    NotStarted,
    /// The daemon validated the launch and the app is spawning the process.
    Launching,
    /// The owned process is alive.
    Running,
    /// The owned process has exited or failed to start.
    Exited,
}

/// One fact read from the daemon about the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptObservation {
    /// Still waiting for the receiver.
    Pending,
    /// The addressed agent's wrapper consumed the handoff.
    DeliveredByTarget,
    /// Resolved by a principal other than the addressed agent.
    ResolvedElsewhere,
    /// Superseded or expired.
    Expired,
    /// Rejected.
    Rejected,
    /// No such receipt for this project.
    NotFound,
    /// The read itself failed.
    ReadFailed {
        /// Short reason for the banner.
        reason: String,
    },
}

/// Anything that can change a [`HandoffView`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    /// The user asked to queue the handoff.
    QueueRequested,
    /// The daemon accepted the handoff.
    QueueSucceeded {
        /// Approval identifier returned by the daemon.
        approval_id: String,
    },
    /// The daemon rejected the handoff or could not be reached.
    QueueFailed,
    /// The user asked to start the receiver.
    LaunchRequested,
    /// The daemon or the OS refused to start the receiver.
    LaunchFailed,
    /// The owned receiver process is alive.
    ReceiverRunning,
    /// The owned receiver process exited.
    ReceiverExited,
    /// A fresh read of the receipt.
    Receipt(ReceiptObservation),
}

/// Everything the inspector needs to render one handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffView {
    /// Receipt evidence.
    pub receipt: ReceiptState,
    /// Receiver liveness.
    pub receiver: ReceiverState,
    /// True when the latest receipt read is newer than the last receiver exit or failed launch.
    pub receipt_is_fresh: bool,
}

impl HandoffView {
    /// A view with nothing queued.
    pub fn new() -> Self {
        Self {
            receipt: ReceiptState::NotQueued,
            receiver: ReceiverState::NotStarted,
            receipt_is_fresh: false,
        }
    }
}

impl Default for HandoffView {
    fn default() -> Self {
        Self::new()
    }
}

/// Applies one observation and returns the new view. See the module's transition table.
pub fn next(view: &HandoffView, observation: &Observation) -> HandoffView {
    let mut out = view.clone();
    match observation {
        Observation::QueueRequested if view.receipt == ReceiptState::NotQueued => {
            out.receipt = ReceiptState::Queuing;
        }
        Observation::QueueSucceeded { approval_id } if view.receipt == ReceiptState::Queuing => {
            out.receipt = ReceiptState::Pending {
                approval_id: approval_id.clone(),
            };
            out.receipt_is_fresh = true;
        }
        Observation::QueueFailed if view.receipt == ReceiptState::Queuing => {
            out.receipt = ReceiptState::NotQueued;
        }
        Observation::LaunchRequested if can_start_receiver(view) => {
            out.receiver = ReceiverState::Launching;
            out.receipt_is_fresh = false;
        }
        Observation::ReceiverRunning if view.receiver == ReceiverState::Launching => {
            out.receiver = ReceiverState::Running;
        }
        Observation::LaunchFailed if view.receiver == ReceiverState::Launching => {
            out.receiver = ReceiverState::Exited;
            out.receipt_is_fresh = false;
        }
        Observation::ReceiverExited
            if matches!(
                view.receiver,
                ReceiverState::Launching | ReceiverState::Running
            ) =>
        {
            out.receiver = ReceiverState::Exited;
            out.receipt_is_fresh = false;
        }
        Observation::Receipt(read) => return apply_receipt(view, read),
        _ => {}
    }
    out
}

/// Applies one receipt read (row 8 of the transition table). Reads are ignored before an
/// approval id exists and after the receipt is terminal.
fn apply_receipt(view: &HandoffView, read: &ReceiptObservation) -> HandoffView {
    let approval_id = match &view.receipt {
        ReceiptState::Pending { approval_id }
        | ReceiptState::Missing { approval_id }
        | ReceiptState::Unreadable { approval_id, .. } => approval_id.clone(),
        _ => return view.clone(),
    };
    let (receipt, fresh) = match read {
        ReceiptObservation::Pending => (ReceiptState::Pending { approval_id }, true),
        ReceiptObservation::DeliveredByTarget => (
            ReceiptState::Delivered { approval_id },
            view.receipt_is_fresh,
        ),
        ReceiptObservation::ResolvedElsewhere => (
            ReceiptState::ResolvedElsewhere { approval_id },
            view.receipt_is_fresh,
        ),
        ReceiptObservation::Expired => {
            (ReceiptState::Expired { approval_id }, view.receipt_is_fresh)
        }
        ReceiptObservation::Rejected => (
            ReceiptState::Rejected { approval_id },
            view.receipt_is_fresh,
        ),
        ReceiptObservation::NotFound => (ReceiptState::Missing { approval_id }, false),
        ReceiptObservation::ReadFailed { reason } => (
            ReceiptState::Unreadable {
                approval_id,
                reason: reason.clone(),
            },
            false,
        ),
    };
    HandoffView {
        receipt,
        receiver: view.receiver,
        receipt_is_fresh: fresh,
    }
}

/// True when the receipt is a freshly read `Pending` and no owned receiver is starting or
/// running. This gates both the first start and every restart.
pub fn can_start_receiver(view: &HandoffView) -> bool {
    matches!(view.receipt, ReceiptState::Pending { .. })
        && view.receipt_is_fresh
        && matches!(
            view.receiver,
            ReceiverState::NotStarted | ReceiverState::Exited
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> String {
        "a1".to_string()
    }

    fn pending() -> HandoffView {
        HandoffView {
            receipt: ReceiptState::Pending { approval_id: id() },
            receiver: ReceiverState::NotStarted,
            receipt_is_fresh: true,
        }
    }

    fn apply(view: HandoffView, observations: &[Observation]) -> HandoffView {
        observations
            .iter()
            .fold(view, |current, observation| next(&current, observation))
    }

    #[test]
    fn a_new_view_has_nothing_queued() {
        let view = HandoffView::new();
        assert_eq!(view.receipt, ReceiptState::NotQueued);
        assert_eq!(view.receiver, ReceiverState::NotStarted);
        assert!(!view.receipt_is_fresh);
        assert_eq!(view, HandoffView::default());
    }

    #[test]
    fn exactly_four_receipt_states_are_terminal() {
        let terminal = [
            ReceiptState::Delivered { approval_id: id() },
            ReceiptState::ResolvedElsewhere { approval_id: id() },
            ReceiptState::Expired { approval_id: id() },
            ReceiptState::Rejected { approval_id: id() },
        ];
        assert!(terminal.iter().all(ReceiptState::is_terminal));

        let recoverable = [
            ReceiptState::NotQueued,
            ReceiptState::Queuing,
            ReceiptState::Pending { approval_id: id() },
            ReceiptState::Missing { approval_id: id() },
            ReceiptState::Unreadable {
                approval_id: id(),
                reason: "timeout".into(),
            },
        ];
        assert!(recoverable.iter().all(|state| !state.is_terminal()));
    }

    // The tests below are the executable specification of the transition table in the module
    // documentation. They pin every row, the terminal-state rule and the delivery attribution.

    #[test]
    fn queueing_reaches_a_fresh_pending_receipt() {
        let view = apply(
            HandoffView::new(),
            &[
                Observation::QueueRequested,
                Observation::QueueSucceeded { approval_id: id() },
            ],
        );
        assert_eq!(view, pending());
    }

    #[test]
    fn a_failed_queue_returns_to_not_queued() {
        let view = apply(
            HandoffView::new(),
            &[Observation::QueueRequested, Observation::QueueFailed],
        );
        assert_eq!(view, HandoffView::new());
    }

    #[test]
    fn starting_the_receiver_consumes_the_fresh_read() {
        let start = pending();
        assert!(can_start_receiver(&start));

        let launching = next(&start, &Observation::LaunchRequested);
        assert_eq!(launching.receiver, ReceiverState::Launching);
        assert!(!launching.receipt_is_fresh);
        assert!(!can_start_receiver(&launching));
    }

    #[test]
    fn an_early_exit_blocks_restart_until_a_fresh_pending_read() {
        let exited = apply(
            pending(),
            &[
                Observation::LaunchRequested,
                Observation::ReceiverRunning,
                Observation::ReceiverExited,
            ],
        );
        assert_eq!(exited.receiver, ReceiverState::Exited);
        assert!(!can_start_receiver(&exited));

        let reread = next(&exited, &Observation::Receipt(ReceiptObservation::Pending));
        assert!(can_start_receiver(&reread));
    }

    #[test]
    fn a_failed_launch_leaves_the_receiver_exited_and_the_read_stale() {
        let failed = apply(
            pending(),
            &[Observation::LaunchRequested, Observation::LaunchFailed],
        );
        assert_eq!(failed.receiver, ReceiverState::Exited);
        assert!(!failed.receipt_is_fresh);
    }

    #[test]
    fn terminal_receipts_cannot_be_undone_by_later_observations() {
        let terminal = [
            ReceiptState::Delivered { approval_id: id() },
            ReceiptState::ResolvedElsewhere { approval_id: id() },
            ReceiptState::Expired { approval_id: id() },
            ReceiptState::Rejected { approval_id: id() },
        ];
        let later = [
            Observation::Receipt(ReceiptObservation::Pending),
            Observation::Receipt(ReceiptObservation::NotFound),
            Observation::Receipt(ReceiptObservation::ReadFailed {
                reason: "timeout".into(),
            }),
            Observation::Receipt(ReceiptObservation::DeliveredByTarget),
            Observation::Receipt(ReceiptObservation::Expired),
            Observation::Receipt(ReceiptObservation::Rejected),
            Observation::Receipt(ReceiptObservation::ResolvedElsewhere),
        ];
        for receipt in terminal {
            let view = HandoffView {
                receipt,
                receiver: ReceiverState::Exited,
                receipt_is_fresh: true,
            };
            for observation in &later {
                assert_eq!(next(&view, observation), view, "{observation:?}");
            }
            assert!(!can_start_receiver(&view));
        }
    }

    #[test]
    fn only_target_delivery_counts_as_delivered() {
        let elsewhere = next(
            &pending(),
            &Observation::Receipt(ReceiptObservation::ResolvedElsewhere),
        );
        assert_eq!(
            elsewhere.receipt,
            ReceiptState::ResolvedElsewhere { approval_id: id() }
        );
        assert!(!can_start_receiver(&elsewhere));

        let delivered = next(
            &pending(),
            &Observation::Receipt(ReceiptObservation::DeliveredByTarget),
        );
        assert_eq!(
            delivered.receipt,
            ReceiptState::Delivered { approval_id: id() }
        );
    }

    #[test]
    fn missing_and_unreadable_receipts_recover_on_a_pending_read() {
        let missing = next(
            &pending(),
            &Observation::Receipt(ReceiptObservation::NotFound),
        );
        assert_eq!(missing.receipt, ReceiptState::Missing { approval_id: id() });
        assert!(!missing.receipt_is_fresh);
        assert!(!can_start_receiver(&missing));

        let unreadable = next(
            &pending(),
            &Observation::Receipt(ReceiptObservation::ReadFailed {
                reason: "timeout".into(),
            }),
        );
        assert_eq!(
            unreadable.receipt,
            ReceiptState::Unreadable {
                approval_id: id(),
                reason: "timeout".into(),
            }
        );

        for broken in [missing, unreadable] {
            let recovered = next(&broken, &Observation::Receipt(ReceiptObservation::Pending));
            assert_eq!(
                recovered.receipt,
                ReceiptState::Pending { approval_id: id() }
            );
            assert!(recovered.receipt_is_fresh);
        }
    }

    #[test]
    fn observations_that_do_not_apply_change_nothing() {
        let blank = HandoffView::new();
        for observation in [
            Observation::Receipt(ReceiptObservation::Pending),
            Observation::LaunchRequested,
            Observation::ReceiverRunning,
            Observation::ReceiverExited,
            Observation::QueueFailed,
            Observation::QueueSucceeded { approval_id: id() },
        ] {
            assert_eq!(next(&blank, &observation), blank, "{observation:?}");
        }
        assert!(!can_start_receiver(&blank));

        let queuing = next(&blank, &Observation::QueueRequested);
        assert_eq!(
            next(&queuing, &Observation::Receipt(ReceiptObservation::Pending)),
            queuing
        );
        assert_eq!(next(&queuing, &Observation::LaunchRequested), queuing);
    }
}
