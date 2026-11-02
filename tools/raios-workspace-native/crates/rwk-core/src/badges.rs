//! Project badges: the three per-project signals a project list can show
//! beside a row — uncommitted files, the health grade, and pending approvals.
//!
//! This module is the contract only: typed readings, a source seam, and a
//! scriptable fake. No source reads git, the CLI, or the daemon yet, and no
//! view renders these readings yet; both land behind [`ProjectBadgeSource`] so
//! the UI never learns where a badge came from.
//!
//! A badge is deliberately not a boolean. "No pending approvals" (`count: 0`)
//! and "the approvals could not be read" are different answers, and a project
//! row that cannot be checked must not look clean: the first is
//! [`BadgeState::Known`], the second [`BadgeState::Unavailable`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Which project signal a badge reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BadgeKind {
    /// Uncommitted files in the project's working tree.
    DirtyFiles,
    /// The project's health/compliance grade.
    Health,
    /// Approvals waiting for the owner's decision.
    PendingApprovals,
}

impl BadgeKind {
    /// Every badge a project row can carry, in display order.
    pub const ALL: [Self; 3] = [Self::DirtyFiles, Self::Health, Self::PendingApprovals];

    /// Stable identifier for logs, tests, and view keys.
    pub fn key(self) -> &'static str {
        match self {
            Self::DirtyFiles => "dirty_files",
            Self::Health => "health",
            Self::PendingApprovals => "pending_approvals",
        }
    }
}

/// The project health grade, as the health scan reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthGrade {
    /// Best grade.
    A,
    /// Good.
    B,
    /// Acceptable.
    C,
    /// Weak.
    D,
    /// Failing.
    F,
    /// The source reported a grade this build does not know.
    Unknown,
}

impl HealthGrade {
    /// Parses the health scan's one-letter grade; anything else is [`Self::Unknown`]
    /// rather than a made-up grade.
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "A" | "a" => Self::A,
            "B" | "b" => Self::B,
            "C" | "c" => Self::C,
            "D" | "d" => Self::D,
            "F" | "f" => Self::F,
            _ => Self::Unknown,
        }
    }

    /// The grade as one letter, `unknown` when it could not be parsed.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::F => "F",
            Self::Unknown => "unknown",
        }
    }
}

/// A reading that answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeValue {
    /// The working tree has uncommitted changes; the count is present only when
    /// the source counted them.
    DirtyFiles {
        /// True when at least one uncommitted change exists.
        dirty: bool,
        /// Number of changed files, when known.
        changed_files: Option<usize>,
    },
    /// The project's health grade, with the numeric score when the scan has one.
    Health {
        /// One-letter grade.
        grade: HealthGrade,
        /// Compliance score, 0–100, when the scan produced one.
        score: Option<u8>,
    },
    /// How many approvals wait for the owner's decision. Zero is a reading.
    PendingApprovals {
        /// Pending approval count.
        count: usize,
    },
}

impl BadgeValue {
    /// Which badge this value belongs to.
    pub fn kind(&self) -> BadgeKind {
        match self {
            Self::DirtyFiles { .. } => BadgeKind::DirtyFiles,
            Self::Health { .. } => BadgeKind::Health,
            Self::PendingApprovals { .. } => BadgeKind::PendingApprovals,
        }
    }
}

/// What one badge shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BadgeState {
    /// The source answered.
    Known(BadgeValue),
    /// The source could not read this signal; the reason is for the journal or a
    /// tooltip, and must never be rendered as a zero value.
    Unavailable {
        /// Why the reading failed.
        reason: String,
    },
}

impl BadgeState {
    /// The reading, when there is one.
    pub fn value(&self) -> Option<&BadgeValue> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unavailable { .. } => None,
        }
    }

    /// True when the signal could not be read.
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}

/// One badge of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBadge {
    /// Which signal this badge reports.
    pub kind: BadgeKind,
    /// The reading or the reason it is missing.
    pub state: BadgeState,
}

impl ProjectBadge {
    /// A badge built from a reading; the kind is the reading's own, so a badge
    /// can never disagree with its value.
    pub fn known(value: BadgeValue) -> Self {
        Self {
            kind: value.kind(),
            state: BadgeState::Known(value),
        }
    }

    /// A badge whose signal could not be read.
    pub fn unavailable(kind: BadgeKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            state: BadgeState::Unavailable {
                reason: reason.into(),
            },
        }
    }
}

/// Every badge read for one project, in [`BadgeKind::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBadges {
    /// Project root the readings belong to.
    pub root: PathBuf,
    badges: Vec<ProjectBadge>,
}

impl ProjectBadges {
    /// Builds the set from whatever the source answered.
    ///
    /// A later reading of the same kind replaces an earlier one (a source that
    /// retries does not produce duplicates), and the result is ordered by
    /// [`BadgeKind::ALL`] regardless of the order the source reported. Kinds the
    /// source did not answer at all stay absent — [`Self::get`] returns `None`,
    /// which is different from an explicit [`BadgeState::Unavailable`].
    pub fn new(root: impl Into<PathBuf>, readings: impl IntoIterator<Item = ProjectBadge>) -> Self {
        let mut kept: BTreeMap<BadgeKind, ProjectBadge> = BTreeMap::new();
        for badge in readings {
            kept.insert(badge.kind, badge);
        }
        let badges = BadgeKind::ALL
            .iter()
            .filter_map(|kind| kept.remove(kind))
            .collect();
        Self {
            root: root.into(),
            badges,
        }
    }

    /// The whole-source failure: every badge is explicitly unavailable for the
    /// same reason, so a row never mistakes "not read" for "clean".
    pub fn unavailable(root: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self::new(
            root,
            BadgeKind::ALL.map(|kind| ProjectBadge::unavailable(kind, reason.clone())),
        )
    }

    /// The badges that were read (or reported unavailable), in display order.
    pub fn badges(&self) -> &[ProjectBadge] {
        &self.badges
    }

    /// One badge by kind; `None` when the source did not answer for it.
    pub fn get(&self, kind: BadgeKind) -> Option<&ProjectBadge> {
        self.badges.iter().find(|badge| badge.kind == kind)
    }

    /// The reading for one kind, when it answered.
    pub fn value(&self, kind: BadgeKind) -> Option<&BadgeValue> {
        self.get(kind)?.state.value()
    }
}

/// Reads the badge signals for a project.
///
/// The trait never returns an error: one unreadable signal is
/// [`BadgeState::Unavailable`], and a whole-source failure is
/// [`ProjectBadges::unavailable`], so a project row always renders. Implementations
/// run off the UI thread; the UI only renders the returned set.
pub trait ProjectBadgeSource: Send + Sync {
    /// Reads the badges of `project`.
    fn badges(&self, project: &Path) -> ProjectBadges;
}

#[derive(Debug, Default)]
struct Script {
    readings: BTreeMap<PathBuf, Vec<ProjectBadge>>,
    calls: Vec<PathBuf>,
}

/// Scriptable [`ProjectBadgeSource`] for tests and for UI work before the real
/// readers exist.
///
/// Readings are scripted per project; a project with no script returns an empty
/// set (the fake never invents a reading), and every call is recorded so a test
/// can assert what was asked for.
#[derive(Debug, Default)]
pub struct FakeBadgeSource {
    script: Mutex<Script>,
}

impl FakeBadgeSource {
    /// Creates a fake with nothing scripted.
    pub fn new() -> Self {
        Self::default()
    }

    fn script(&self) -> MutexGuard<'_, Script> {
        self.script.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Replaces the reading of one kind for `project`.
    pub fn set(&self, project: impl Into<PathBuf>, badge: ProjectBadge) -> &Self {
        let mut script = self.script();
        let readings = script.readings.entry(project.into()).or_default();
        readings.retain(|existing| existing.kind != badge.kind);
        readings.push(badge);
        self
    }

    /// Scripts the whole-source failure for `project`.
    pub fn fail(&self, project: impl Into<PathBuf>, reason: impl Into<String>) -> &Self {
        let project = project.into();
        let reason = reason.into();
        let mut script = self.script();
        script.readings.insert(
            project,
            BadgeKind::ALL
                .map(|kind| ProjectBadge::unavailable(kind, reason.clone()))
                .to_vec(),
        );
        self
    }

    /// The projects this fake was asked about, in call order.
    pub fn calls(&self) -> Vec<PathBuf> {
        self.script().calls.clone()
    }
}

impl ProjectBadgeSource for FakeBadgeSource {
    fn badges(&self, project: &Path) -> ProjectBadges {
        let mut script = self.script();
        script.calls.push(project.to_path_buf());
        let readings = script.readings.get(project).cloned().unwrap_or_default();
        ProjectBadges::new(project, readings)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn dirty(dirty: bool, changed_files: Option<usize>) -> BadgeValue {
        BadgeValue::DirtyFiles {
            dirty,
            changed_files,
        }
    }

    #[test]
    fn each_value_belongs_to_exactly_one_kind() {
        assert_eq!(dirty(true, None).kind(), BadgeKind::DirtyFiles);
        assert_eq!(
            BadgeValue::Health {
                grade: HealthGrade::A,
                score: Some(96)
            }
            .kind(),
            BadgeKind::Health
        );
        assert_eq!(
            BadgeValue::PendingApprovals { count: 2 }.kind(),
            BadgeKind::PendingApprovals
        );
    }

    #[test]
    fn grades_parse_their_letter_and_never_invent_one() {
        for (raw, grade) in [
            ("A", HealthGrade::A),
            ("b", HealthGrade::B),
            (" C ", HealthGrade::C),
            ("D", HealthGrade::D),
            ("F", HealthGrade::F),
            ("A+", HealthGrade::Unknown),
            ("", HealthGrade::Unknown),
        ] {
            assert_eq!(HealthGrade::parse(raw), grade, "raw grade {raw:?}");
        }
        assert_eq!(HealthGrade::A.as_str(), "A");
        assert_eq!(HealthGrade::Unknown.as_str(), "unknown");
    }

    #[test]
    fn readings_are_ordered_by_kind_not_by_arrival() {
        let badges = ProjectBadges::new(
            "/w/raios",
            [
                ProjectBadge::known(BadgeValue::PendingApprovals { count: 4 }),
                ProjectBadge::known(dirty(true, Some(3))),
                ProjectBadge::known(BadgeValue::Health {
                    grade: HealthGrade::B,
                    score: None,
                }),
            ],
        );
        let kinds: Vec<BadgeKind> = badges.badges().iter().map(|badge| badge.kind).collect();
        assert_eq!(kinds, BadgeKind::ALL);
        assert_eq!(badges.root, PathBuf::from("/w/raios"));
    }

    #[test]
    fn a_retried_reading_replaces_the_earlier_one() {
        let badges = ProjectBadges::new(
            "/w/raios",
            [
                ProjectBadge::known(dirty(true, None)),
                ProjectBadge::known(dirty(false, Some(0))),
            ],
        );
        assert_eq!(badges.badges().len(), 1);
        assert_eq!(
            badges.value(BadgeKind::DirtyFiles),
            Some(&dirty(false, Some(0)))
        );
    }

    /// Zero is a reading, not a missing answer: an empty approvals list renders
    /// as "0", while a failed read must not.
    #[test]
    fn zero_and_unavailable_are_different_answers() {
        let clean = ProjectBadges::new(
            "/w/raios",
            [ProjectBadge::known(BadgeValue::PendingApprovals {
                count: 0,
            })],
        );
        assert_eq!(
            clean.value(BadgeKind::PendingApprovals),
            Some(&BadgeValue::PendingApprovals { count: 0 })
        );
        assert!(!clean
            .get(BadgeKind::PendingApprovals)
            .unwrap()
            .state
            .is_unavailable());

        let failed = ProjectBadges::unavailable("/w/raios", "daemon unreachable");
        let badge = failed.get(BadgeKind::PendingApprovals).expect("reported");
        assert!(badge.state.is_unavailable());
        assert_eq!(failed.value(BadgeKind::PendingApprovals), None);
        assert_eq!(
            failed.badges().len(),
            BadgeKind::ALL.len(),
            "a whole-source failure answers for every badge explicitly"
        );
    }

    #[test]
    fn an_unanswered_kind_stays_absent_instead_of_looking_clean() {
        let badges = ProjectBadges::new(
            "/w/raios",
            [ProjectBadge::known(BadgeValue::PendingApprovals {
                count: 1,
            })],
        );
        assert_eq!(badges.value(BadgeKind::Health), None);
        assert!(badges.get(BadgeKind::Health).is_none());
        assert_eq!(badges.badges().len(), 1);
    }

    #[test]
    fn a_known_badge_cannot_disagree_with_its_value() {
        let badge = ProjectBadge::known(BadgeValue::Health {
            grade: HealthGrade::F,
            score: Some(12),
        });
        assert_eq!(badge.kind, BadgeKind::Health);
        assert_eq!(
            badge.state.value(),
            Some(&BadgeValue::Health {
                grade: HealthGrade::F,
                score: Some(12)
            })
        );
    }

    #[test]
    fn the_fake_answers_per_project_and_records_every_call() {
        let fake = FakeBadgeSource::new();
        fake.set(
            "/w/raios",
            ProjectBadge::known(BadgeValue::PendingApprovals { count: 3 }),
        );
        fake.set("/w/raios", ProjectBadge::known(dirty(true, Some(7))));

        let raios = fake.badges(Path::new("/w/raios"));
        assert_eq!(raios.badges().len(), 2);
        assert_eq!(
            raios.value(BadgeKind::DirtyFiles),
            Some(&dirty(true, Some(7)))
        );
        assert_eq!(raios.value(BadgeKind::Health), None);

        let other = fake.badges(Path::new("/w/other"));
        assert!(other.badges().is_empty());

        assert_eq!(
            fake.calls(),
            vec![PathBuf::from("/w/raios"), PathBuf::from("/w/other")]
        );
    }

    #[test]
    fn a_repeated_set_replaces_the_kind_in_the_fake() {
        let fake = FakeBadgeSource::new();
        fake.set(
            "/w/raios",
            ProjectBadge::known(BadgeValue::PendingApprovals { count: 5 }),
        );
        fake.set(
            "/w/raios",
            ProjectBadge::known(BadgeValue::PendingApprovals { count: 0 }),
        );

        let badges = fake.badges(Path::new("/w/raios"));
        assert_eq!(badges.badges().len(), 1);
        assert_eq!(
            badges.value(BadgeKind::PendingApprovals),
            Some(&BadgeValue::PendingApprovals { count: 0 })
        );
    }

    #[test]
    fn the_fake_can_script_a_whole_source_failure() {
        let fake = FakeBadgeSource::new();
        fake.fail("/w/raios", "raios health timed out");
        let badges = fake.badges(Path::new("/w/raios"));
        assert_eq!(badges.badges().len(), BadgeKind::ALL.len());
        assert!(badges
            .badges()
            .iter()
            .all(|badge| badge.state.is_unavailable()));
    }

    /// The seam is what the UI will hold: a trait object behind an `Arc`, shared
    /// across threads, with no production reader required to exercise it.
    #[test]
    fn the_source_seam_is_shareable_across_threads() {
        let source: Arc<dyn ProjectBadgeSource> = Arc::new(FakeBadgeSource::new());
        let worker = std::thread::spawn(move || source.badges(Path::new("/w/raios")));
        let badges = worker.join().expect("no panic in the reading thread");
        assert!(badges.badges().is_empty());
    }
}
