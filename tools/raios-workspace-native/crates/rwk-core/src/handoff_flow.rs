//! Glue between the daemon's handoff receipt and the pure lifecycle in [`crate::handoff_state`]:
//! turning a receipt read into an observation, tracking one queued handoff, and the words the
//! inspector shows for it.

use raios_contracts::HandoffReceiptDto;

use crate::error::{ErrorCode, Result};
use crate::handoff_state::{
    can_start_receiver, next, HandoffView, Observation, ReceiptObservation, ReceiptState,
    ReceiverState,
};

/// Turns one receipt read into the fact the state machine understands.
///
/// A receipt for another approval id, or a status this client does not know, counts as a failed
/// read: delivery is never claimed from data that does not clearly say so.
pub fn observe_receipt(
    result: &Result<HandoffReceiptDto>,
    expected_id: &str,
) -> ReceiptObservation {
    let receipt = match result {
        Ok(receipt) => receipt,
        Err(error) if error.code == ErrorCode::NotFound => return ReceiptObservation::NotFound,
        Err(error) => {
            return ReceiptObservation::ReadFailed {
                reason: error.message.clone(),
            }
        }
    };
    if receipt.approval_id != expected_id {
        return ReceiptObservation::ReadFailed {
            reason: "the daemon answered for a different handoff".to_owned(),
        };
    }
    match receipt.status.as_str() {
        "pending" => ReceiptObservation::Pending,
        "approved" if receipt.delivered_by_target => ReceiptObservation::DeliveredByTarget,
        "approved" => ReceiptObservation::ResolvedElsewhere,
        "rejected" => ReceiptObservation::Rejected,
        "expired" => ReceiptObservation::Expired,
        other => ReceiptObservation::ReadFailed {
            reason: format!("unknown receipt status '{other}'"),
        },
    }
}

/// The task a handoff carries. It moves to the receiver only after the receipt says delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLink {
    /// Task identifier.
    pub task_id: String,
    /// Title, for the status line.
    pub title: String,
}

/// The text a handoff note starts with: where it comes from and, when there is one, the task.
/// The user edits it before sending; the usual validation still applies to what is sent.
pub fn handoff_seed(sender_label: &str, task_title: Option<&str>) -> String {
    let mut seed = format!("Devreden terminal: {sender_label}\n");
    if let Some(title) = task_title {
        seed.push_str(&format!("Görev: {title}\n"));
    }
    seed.push_str("Durum ve sıradaki adım: ");
    seed
}

/// One handoff this workspace queued, with its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedHandoff {
    /// Approval identifier returned by the daemon.
    pub approval_id: String,
    /// Agent identity that handed off.
    pub from: String,
    /// Agent identity it is addressed to.
    pub to: String,
    /// Lifecycle of the receipt and of an owned receiver.
    pub view: HandoffView,
    /// The task that follows the handoff, once it is delivered.
    pub task: Option<TaskLink>,
    /// True once the task was handed to the receiver (or the attempt was made).
    pub task_moved: bool,
}

impl TrackedHandoff {
    /// A handoff the daemon has just accepted.
    pub fn queued(approval_id: String, from: String, to: String) -> Self {
        let view = next(
            &next(&HandoffView::new(), &Observation::QueueRequested),
            &Observation::QueueSucceeded {
                approval_id: approval_id.clone(),
            },
        );
        Self {
            approval_id,
            from,
            to,
            view,
            task: None,
            task_moved: false,
        }
    }

    /// The same handoff, carrying a task.
    pub fn with_task(mut self, task: Option<TaskLink>) -> Self {
        self.task = task;
        self
    }

    /// True when the receipt proves delivery and the task has not been moved yet.
    pub fn task_move_due(&self) -> bool {
        self.task.is_some()
            && !self.task_moved
            && matches!(self.view.receipt, ReceiptState::Delivered { .. })
    }

    /// Applies one observation.
    pub fn observe(&mut self, observation: &Observation) {
        self.view = next(&self.view, observation);
    }

    /// True when "start the receiver" may be offered right now.
    pub fn can_start_receiver(&self) -> bool {
        can_start_receiver(&self.view)
    }

    /// True while polling the receipt still makes sense.
    pub fn is_open(&self) -> bool {
        !self.view.receipt.is_terminal()
    }

    /// The line the inspector shows.
    pub fn label(&self) -> String {
        format!("{} → {} · {}", self.from, self.to, receipt_text(&self.view))
    }
}

/// What the receipt and the receiver mean for the user.
pub fn receipt_text(view: &HandoffView) -> String {
    let receipt = match &view.receipt {
        ReceiptState::NotQueued => "kuyrukta değil",
        ReceiptState::Queuing => "kuyruğa alınıyor",
        ReceiptState::Pending { .. } => "alıcıyı bekliyor",
        ReceiptState::Delivered { .. } => "teslim edildi",
        ReceiptState::ResolvedElsewhere { .. } => "başka biri tarafından kapatıldı (teslim değil)",
        ReceiptState::Expired { .. } => "süresi doldu",
        ReceiptState::Rejected { .. } => "reddedildi",
        ReceiptState::Missing { .. } => "makbuz bulunamadı",
        ReceiptState::Unreadable { .. } => "makbuz okunamadı",
    };
    let receiver = match view.receiver {
        ReceiverState::NotStarted => "",
        ReceiverState::Launching => " · alıcı başlatılıyor",
        ReceiverState::Running => " · alıcı çalışıyor",
        ReceiverState::Exited => " · alıcı çıktı",
    };
    format!("{receipt}{receiver}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    fn receipt(status: &str, delivered: bool) -> HandoffReceiptDto {
        HandoffReceiptDto {
            approval_id: "a-1".into(),
            project_path: "/work/app".into(),
            from_agent: "claude_kaira".into(),
            to_agent: "codex_kaira".into(),
            status: status.into(),
            resolved_by: None,
            resolved_at: None,
            delivered_by_target: delivered,
        }
    }

    fn observe(dto: HandoffReceiptDto) -> ReceiptObservation {
        observe_receipt(&Ok(dto), "a-1")
    }

    #[test]
    fn statuses_map_to_observations() {
        assert_eq!(
            observe(receipt("pending", false)),
            ReceiptObservation::Pending
        );
        assert_eq!(
            observe(receipt("rejected", false)),
            ReceiptObservation::Rejected
        );
        assert_eq!(
            observe(receipt("expired", false)),
            ReceiptObservation::Expired
        );
    }

    #[test]
    fn only_the_targets_own_wrapper_counts_as_delivery() {
        assert_eq!(
            observe(receipt("approved", true)),
            ReceiptObservation::DeliveredByTarget
        );
        assert_eq!(
            observe(receipt("approved", false)),
            ReceiptObservation::ResolvedElsewhere
        );
    }

    #[test]
    fn unknown_statuses_and_foreign_receipts_never_claim_delivery() {
        assert!(matches!(
            observe(receipt("teleported", true)),
            ReceiptObservation::ReadFailed { .. }
        ));
        let mut other = receipt("approved", true);
        other.approval_id = "a-2".into();
        assert!(matches!(
            observe(other),
            ReceiptObservation::ReadFailed { .. }
        ));
    }

    #[test]
    fn a_missing_receipt_and_a_failed_read_are_told_apart() {
        let missing = Err(AppError::new(ErrorCode::NotFound, "no such receipt"));
        assert_eq!(
            observe_receipt(&missing, "a-1"),
            ReceiptObservation::NotFound
        );
        let down = Err(AppError::new(ErrorCode::DaemonUnreachable, "refused"));
        assert!(matches!(
            observe_receipt(&down, "a-1"),
            ReceiptObservation::ReadFailed { .. }
        ));
    }

    #[test]
    fn a_queued_handoff_waits_for_its_receiver_and_can_start_it() {
        let tracked =
            TrackedHandoff::queued("a-1".into(), "claude_kaira".into(), "codex_kaira".into());

        assert!(tracked.can_start_receiver());
        assert!(tracked.is_open());
        assert!(
            tracked.label().contains("alıcıyı bekliyor"),
            "{}",
            tracked.label()
        );
    }

    #[test]
    fn the_task_follows_only_a_delivered_handoff_and_only_once() {
        let link = TaskLink {
            task_id: "t-1".into(),
            title: "Derle".into(),
        };
        let mut tracked =
            TrackedHandoff::queued("a-1".into(), "c".into(), "x".into()).with_task(Some(link));
        assert!(!tracked.task_move_due());

        tracked.observe(&Observation::Receipt(ReceiptObservation::ResolvedElsewhere));
        assert!(!tracked.task_move_due(), "approval is not delivery");

        let mut tracked = TrackedHandoff::queued("a-2".into(), "c".into(), "x".into()).with_task(
            Some(TaskLink {
                task_id: "t-1".into(),
                title: "Derle".into(),
            }),
        );
        tracked.observe(&Observation::Receipt(ReceiptObservation::DeliveredByTarget));
        assert!(tracked.task_move_due());
        tracked.task_moved = true;
        assert!(!tracked.task_move_due());
    }

    #[test]
    fn the_seed_names_the_sender_and_the_task_and_passes_validation() {
        let seed = handoff_seed("derleme", Some("Testleri düzelt"));
        assert!(seed.contains("derleme") && seed.contains("Testleri düzelt"));
        let text = seed.trim();
        assert!(crate::handoff_input::validate_summary(
            text,
            &crate::handoff_input::looks_like_secret
        )
        .is_ok());
        assert!(!handoff_seed("claude 1", None).contains("Görev:"));
    }

    #[test]
    fn delivery_closes_the_handoff_for_good() {
        let mut tracked = TrackedHandoff::queued("a-1".into(), "c".into(), "x".into());

        tracked.observe(&Observation::Receipt(ReceiptObservation::DeliveredByTarget));
        tracked.observe(&Observation::Receipt(ReceiptObservation::NotFound));

        assert!(!tracked.is_open());
        assert!(!tracked.can_start_receiver());
        assert!(tracked.label().contains("teslim edildi"));
    }

    #[test]
    fn the_receiver_state_is_part_of_the_label_and_blocks_a_second_start() {
        let mut tracked = TrackedHandoff::queued("a-1".into(), "c".into(), "x".into());

        tracked.observe(&Observation::LaunchRequested);

        assert!(!tracked.can_start_receiver());
        assert!(tracked.label().contains("alıcı başlatılıyor"));
    }
}
