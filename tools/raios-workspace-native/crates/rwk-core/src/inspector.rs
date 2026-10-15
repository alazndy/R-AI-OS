//! The inspector's data: what the daemon knows about the open project.
//!
//! Pure functions over the daemon's unfiltered snapshots. A record is shown only when its
//! `project_path` equals the open project's root exactly; records without a path, or with a path
//! of another project, are omitted (a global or ambiguous record must never leak into a project
//! view). Approvals are read-only here.

use std::path::Path;

use raios_contracts::{ActiveRunDto, NowSnapshot, ScoredApprovalDto, UnifiedTaskDto, WorkSnapshot};

use crate::control::{ConnectionState, ControlPlane};

/// The banner above the inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Banner {
    /// Everything works; nothing is shown.
    None,
    /// Some requests fail, so what follows may be stale.
    Degraded(String),
    /// The daemon cannot be reached; actions that need it are unavailable.
    Unreachable(String),
}

impl Banner {
    /// The banner for a connection state.
    pub fn for_state(state: &ConnectionState) -> Self {
        match state {
            ConnectionState::Connected => Self::None,
            ConnectionState::Degraded { reason } => Self::Degraded(reason.clone()),
            ConnectionState::Unreachable { reason } => Self::Unreachable(reason.clone()),
        }
    }

    /// The text shown to the user, `None` when there is no banner.
    pub fn text(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Degraded(reason) => {
                Some(format!("Daemon yavaş veya kısmen erişilemiyor: {reason}"))
            }
            Self::Unreachable(reason) => Some(format!("Daemon'a ulaşılamıyor: {reason}")),
        }
    }
}

/// One task of the open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    /// Task identifier.
    pub id: String,
    /// Short title.
    pub title: String,
    /// Status label from the daemon.
    pub status: String,
    /// Assigned agent or person, when there is one.
    pub assignee: Option<String>,
}

/// One running agent of the open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    /// Run identifier.
    pub run_id: String,
    /// Agent harness name.
    pub agent: String,
    /// Task the run belongs to.
    pub task_id: String,
}

/// One pending approval of the open project (read-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRow {
    /// Approval identifier.
    pub id: String,
    /// What is being approved.
    pub title: String,
    /// Kind label, for example `handoff`.
    pub kind: String,
    /// Agent that asked.
    pub from_agent: String,
    /// Agent it is addressed to.
    pub to_agent: String,
    /// Why approval is required.
    pub reason: String,
    /// Revision the row was read at; a decision must echo it so the daemon can refuse a
    /// decision based on a stale read.
    pub revision: i64,
}

/// Everything the inspector shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectorModel {
    /// Connection banner.
    pub banner: Banner,
    /// Tasks of the project.
    pub tasks: Vec<TaskRow>,
    /// Running agents of the project.
    pub runs: Vec<RunRow>,
    /// Pending approvals of the project.
    pub approvals: Vec<ApprovalRow>,
    /// How many tasks and approvals carry no project path and are therefore not shown anywhere
    /// near a project; only the number is reported, never their content.
    pub unscoped: usize,
}

impl InspectorModel {
    /// An inspector with nothing to show (before the first snapshot, or no project).
    pub fn empty(state: &ConnectionState) -> Self {
        Self {
            banner: Banner::for_state(state),
            tasks: Vec::new(),
            runs: Vec::new(),
            approvals: Vec::new(),
            unscoped: 0,
        }
    }

    /// True when there is nothing to list.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty() && self.runs.is_empty() && self.approvals.is_empty()
    }
}

fn in_project(path: Option<&str>, project: &Path) -> bool {
    path.is_some_and(|path| Path::new(path) == project)
}

fn project_runs<'a>(
    runs: &'a [ActiveRunDto],
    tasks: &'a [UnifiedTaskDto],
    project: &'a Path,
) -> impl Iterator<Item = &'a ActiveRunDto> {
    // A run carries no path of its own: it belongs to the project when its task does.
    runs.iter().filter(move |run| {
        tasks
            .iter()
            .any(|task| task.id == run.task_id && in_project(task.project_path.as_deref(), project))
    })
}

/// The inspector for `project`, from the daemon's unfiltered snapshots.
pub fn project_view(
    work: &WorkSnapshot,
    now: &NowSnapshot,
    project: &Path,
    state: &ConnectionState,
) -> InspectorModel {
    let tasks = work
        .tasks
        .iter()
        .filter(|task| in_project(task.project_path.as_deref(), project))
        .map(|task| TaskRow {
            id: task.id.clone(),
            title: task.title.clone(),
            status: task.status.clone(),
            assignee: task.assignee.clone(),
        })
        .collect();
    let runs = project_runs(&work.active_runs, &work.tasks, project)
        .map(|run| RunRow {
            run_id: run.run_id.clone(),
            agent: run.agent_name.clone(),
            task_id: run.task_id.clone(),
        })
        .collect();
    let approvals = now
        .approvals
        .iter()
        .filter(|approval: &&ScoredApprovalDto| {
            in_project(approval.project_path.as_deref(), project)
        })
        .map(|approval| ApprovalRow {
            id: approval.id.clone(),
            title: approval.title.clone(),
            kind: approval.kind.clone(),
            from_agent: approval.origin_agent.clone(),
            to_agent: approval.target_agent.clone(),
            reason: approval.reason.clone(),
            revision: approval.revision,
        })
        .collect();
    let unscoped = work
        .tasks
        .iter()
        .filter(|task| task.project_path.is_none())
        .count()
        + now
            .approvals
            .iter()
            .filter(|approval| approval.project_path.is_none())
            .count();
    InspectorModel {
        banner: Banner::for_state(state),
        tasks,
        runs,
        approvals,
        unscoped,
    }
}

/// Reads both projections from the daemon and builds the inspector for `project`.
///
/// Blocking: call it from a background executor. When both reads fail the daemon counts as
/// unreachable and nothing is listed; when one fails the other half is shown with a degraded
/// banner; a failed read never leaves stale rows of the failed half.
pub fn fetch(plane: &dyn ControlPlane, project: &Path) -> InspectorModel {
    match (plane.work_snapshot(), plane.now_snapshot()) {
        (Ok(work), Ok(now)) => project_view(&work, &now, project, &ConnectionState::Connected),
        (Ok(work), Err(error)) => project_view(
            &work,
            &NowSnapshot::default(),
            project,
            &ConnectionState::Degraded {
                reason: error.message,
            },
        ),
        (Err(error), Ok(now)) => project_view(
            &WorkSnapshot::default(),
            &now,
            project,
            &ConnectionState::Degraded {
                reason: error.message,
            },
        ),
        (Err(error), Err(_)) => InspectorModel::empty(&ConnectionState::Unreachable {
            reason: error.message,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, path: Option<&str>) -> UnifiedTaskDto {
        UnifiedTaskDto {
            id: id.into(),
            title: format!("task {id}"),
            project_path: path.map(str::to_owned),
            status: "in_progress".into(),
            ..UnifiedTaskDto::default()
        }
    }

    fn run(id: &str, task_id: &str) -> ActiveRunDto {
        ActiveRunDto {
            run_id: id.into(),
            task_id: task_id.into(),
            agent_name: "codex_kaira".into(),
            ..ActiveRunDto::default()
        }
    }

    fn approval(id: &str, path: Option<&str>) -> ScoredApprovalDto {
        ScoredApprovalDto {
            id: id.into(),
            kind: "handoff".into(),
            origin_agent: "claude_kaira".into(),
            target_agent: "codex_kaira".into(),
            project_path: path.map(str::to_owned),
            revision: 7,
            ..ScoredApprovalDto::default()
        }
    }

    fn snapshots() -> (WorkSnapshot, NowSnapshot) {
        let work = WorkSnapshot {
            tasks: vec![
                task("t1", Some("/work/app")),
                task("t2", Some("/work/app-extra")),
                task("t3", None),
                task("t4", Some("/work/app/")),
            ],
            active_runs: vec![run("r1", "t1"), run("r2", "t2"), run("r3", "t3")],
            ..WorkSnapshot::default()
        };
        let now = NowSnapshot {
            approvals: vec![
                approval("a1", Some("/work/app")),
                approval("a2", Some("/work/other")),
                approval("a3", None),
            ],
            ..NowSnapshot::default()
        };
        (work, now)
    }

    #[test]
    fn only_records_of_exactly_this_project_are_shown() {
        let (work, now) = snapshots();

        let model = project_view(
            &work,
            &now,
            Path::new("/work/app"),
            &ConnectionState::Connected,
        );

        let task_ids: Vec<_> = model.tasks.iter().map(|task| task.id.as_str()).collect();
        // `/work/app-extra` is a different project; `/work/app/` is the same path with a
        // trailing slash, which `Path` equality treats as equal.
        assert_eq!(task_ids, ["t1", "t4"]);
        assert_eq!(model.approvals.len(), 1);
        assert_eq!(model.approvals[0].id, "a1");
        // The row carries everything a decision needs: both agents and the revision to echo.
        assert_eq!(model.approvals[0].from_agent, "claude_kaira");
        assert_eq!(model.approvals[0].to_agent, "codex_kaira");
        assert_eq!(model.approvals[0].revision, 7);
    }

    #[test]
    fn records_without_a_path_are_only_counted() {
        let (work, now) = snapshots();

        let model = project_view(
            &work,
            &now,
            Path::new("/work/app"),
            &ConnectionState::Connected,
        );

        // task t3 and approval a3 have no path.
        assert_eq!(model.unscoped, 2);
    }

    #[test]
    fn records_without_a_path_never_leak_into_a_project() {
        let (work, now) = snapshots();

        let model = project_view(
            &work,
            &now,
            Path::new("/work/app"),
            &ConnectionState::Connected,
        );

        assert!(model.tasks.iter().all(|task| task.id != "t3"));
        assert!(model.approvals.iter().all(|approval| approval.id != "a3"));
    }

    #[test]
    fn a_run_belongs_to_the_project_of_its_task() {
        let (work, now) = snapshots();

        let model = project_view(
            &work,
            &now,
            Path::new("/work/app"),
            &ConnectionState::Connected,
        );

        let run_ids: Vec<_> = model.runs.iter().map(|run| run.run_id.as_str()).collect();
        assert_eq!(run_ids, ["r1"]);
    }

    #[test]
    fn another_project_sees_none_of_these_records() {
        let (work, now) = snapshots();

        let model = project_view(
            &work,
            &now,
            Path::new("/nowhere"),
            &ConnectionState::Connected,
        );

        assert!(model.is_empty());
    }

    #[test]
    fn the_banner_follows_the_connection_state() {
        assert_eq!(Banner::for_state(&ConnectionState::Connected).text(), None);
        let degraded = ConnectionState::Degraded {
            reason: "yavaş".into(),
        };
        assert!(Banner::for_state(&degraded)
            .text()
            .is_some_and(|text| text.contains("yavaş")));
        let unreachable = ConnectionState::Unreachable {
            reason: "bağlantı reddedildi".into(),
        };
        assert!(matches!(
            Banner::for_state(&unreachable),
            Banner::Unreachable(_)
        ));
        assert!(InspectorModel::empty(&unreachable).is_empty());
    }

    mod fetch_tests {
        use super::super::*;
        use crate::control::fake::FakeControlPlane;
        use crate::error::{AppError, ErrorCode};

        fn failure() -> AppError {
            AppError::new(ErrorCode::DaemonUnreachable, "bağlantı reddedildi")
        }

        #[test]
        fn both_reads_ok_is_connected() {
            let plane = FakeControlPlane::new();
            plane
                .push_work_snapshot(Ok(WorkSnapshot::default()))
                .push_now_snapshot(Ok(NowSnapshot::default()));

            let model = fetch(&plane, Path::new("/a"));

            assert_eq!(model.banner, Banner::None);
        }

        #[test]
        fn one_failed_read_is_degraded_and_keeps_the_other_half() {
            let plane = FakeControlPlane::new();
            let work = WorkSnapshot {
                tasks: vec![UnifiedTaskDto {
                    id: "t".into(),
                    project_path: Some("/a".into()),
                    ..UnifiedTaskDto::default()
                }],
                ..WorkSnapshot::default()
            };
            plane
                .push_work_snapshot(Ok(work))
                .push_now_snapshot(Err(failure()));

            let model = fetch(&plane, Path::new("/a"));

            assert!(matches!(model.banner, Banner::Degraded(_)));
            assert_eq!(model.tasks.len(), 1);
        }

        #[test]
        fn both_reads_failing_is_unreachable_and_lists_nothing() {
            let plane = FakeControlPlane::new();
            plane
                .push_work_snapshot(Err(failure()))
                .push_now_snapshot(Err(failure()));

            let model = fetch(&plane, Path::new("/a"));

            assert!(matches!(model.banner, Banner::Unreachable(_)));
            assert!(model.is_empty());
        }
    }
}
